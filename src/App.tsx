import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { confirm, open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { UpdateNotice } from "./components/UpdateNotice";
import {
  ArrowRight,
  Check,
  Clock3,
  FileArchive,
  FolderOpen,
  Gauge,
  HardDrive,
  Info,
  Laptop,
  LoaderCircle,
  LockKeyhole,
  RefreshCw,
  ShieldCheck,
  Sparkles,
  Upload,
  Usb,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import merakiFlashIcon from "../src-tauri/icons/icon.png";
import { UnattendPanel } from "./components/UnattendPanel";
import {
  DEFAULT_UNATTEND_OPTIONS,
  generateAutounattendXml,
  validateUnattendOptions,
} from "./lib/unattend";

type OperatingSystem = "windows" | "linux";
type FlashPhase =
  | "idle"
  | "preparing"
  | "analyzing"
  | "splitting"
  | "formatting"
  | "extracting"
  | "writing"
  | "syncing"
  | "verifying"
  | "done"
  | "error";

interface UsbDevice {
  id: string;
  name: string;
  devicePath: string;
  mountPoint: string;
  mountPoints: string[];
  fileSystem: string;
  totalBytes: number;
  availableBytes: number;
  readOnly: boolean;
  kind: string;
  transport: string;
  serial: string | null;
}

interface SelectedIso {
  name: string;
  path: string;
}

interface FlashProgress {
  operationId?: string;
  phase: FlashPhase;
  percentage: number;
  bytesPerSecond: number;
  etaSeconds: number | null;
  message?: string;
}

const EMPTY_PROGRESS: FlashProgress = {
  phase: "idle",
  percentage: 0,
  bytesPerSecond: 0,
  etaSeconds: null,
};

const osOptions = [
  {
    id: "windows" as const,
    title: "Windows",
    description: "Windows 10, 11 e imagens UEFI",
    accent: "from-[#00F0FF] to-[#2878ff]",
    icon: Laptop,
  },
  {
    id: "linux" as const,
    title: "Linux",
    description: "Ubuntu, Fedora, Arch e outras distros",
    accent: "from-[#2878ff] to-[#8A2BE2]",
    icon: Sparkles,
  },
];

const phaseLabels: Record<FlashPhase, string> = {
  idle: "Aguardando configuração",
  preparing: "Preparando o dispositivo",
  analyzing: "Analisando a imagem",
  splitting: "Dividindo install.wim",
  formatting: "Criando a partição FAT32",
  extracting: "Extraindo os arquivos",
  writing: "Gravando a imagem",
  syncing: "Sincronizando os dados",
  verifying: "Verificando a gravação",
  done: "Pendrive pronto",
  error: "A gravação foi interrompida",
};

function App() {
  const [operatingSystem, setOperatingSystem] = useState<OperatingSystem | null>(null);
  const [selectedIso, setSelectedIso] = useState<SelectedIso | null>(null);
  const [devices, setDevices] = useState<UsbDevice[]>([]);
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(null);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [deviceError, setDeviceError] = useState<string | null>(null);
  const [progress, setProgress] = useState<FlashProgress>(EMPTY_PROGRESS);
  const [notice, setNotice] = useState<string | null>(null);
  const [isConfirming, setIsConfirming] = useState(false);
  const [progressReady, setProgressReady] = useState(false);
  const startPending = useRef(false);
  const activeOperation = useRef<string | null>(null);
  const [unattendEnabled, setUnattendEnabled] = useState(false);
  const [unattendOptions, setUnattendOptions] = useState(DEFAULT_UNATTEND_OPTIONS);
  const browserFileInput = useRef<HTMLInputElement>(null);

  const selectedDevice = useMemo(
    () => devices.find((device) => device.id === selectedDeviceId) ?? null,
    [devices, selectedDeviceId],
  );

  const showUnattendPanel = Boolean(
    operatingSystem === "windows" && selectedIso && selectedDevice,
  );
  const unattendError =
    showUnattendPanel && unattendEnabled ? validateUnattendOptions(unattendOptions) : null;
  const isReady = Boolean(
    operatingSystem &&
      selectedIso &&
      selectedDevice &&
      !selectedDevice.readOnly &&
      !unattendError,
  );
  const isBusy = isConfirming || !["idle", "done", "error"].includes(progress.phase);
  const currentStep = progress.phase !== "idle" ? 4 : selectedDevice ? 3 : selectedIso ? 2 : 1;
  const stepSelections = [Boolean(operatingSystem), Boolean(selectedIso), Boolean(selectedDevice) || progress.phase === "done", progress.phase !== "idle" && progress.phase !== "error"];

  const resetProgress = () => {
    setProgress(EMPTY_PROGRESS);
    setNotice(null);
  };

  const refreshDevices = useCallback(async () => {
    setIsRefreshing(true);
    setDeviceError(null);

    try {
      const removableDevices = await invoke<UsbDevice[]>("list_usb_devices");
      setDevices(removableDevices);
      setSelectedDeviceId((current) =>
        removableDevices.some((device) => device.id === current) ? current : null,
      );
    } catch (error) {
      setDevices([]);
      setSelectedDeviceId(null);
      setDeviceError(
        isRunningInTauri()
          ? readableError(error)
          : "A detecção nativa fica disponível ao executar npm run tauri dev.",
      );
    } finally {
      setIsRefreshing(false);
    }
  }, []);

  useEffect(() => {
    void refreshDevices();
  }, [refreshDevices]);

  useEffect(() => {
    if (!isRunningInTauri()) return;

    let unlisten: UnlistenFn | undefined;
    let disposed = false;
    void listen<FlashProgress>("flash-progress", (event) => {
      if (!startPending.current || (activeOperation.current && event.payload.operationId !== activeOperation.current)) return;
      activeOperation.current = event.payload.operationId ?? null;
      setProgress(event.payload);
      setNotice(null);
      if (event.payload.phase === "done") {
        startPending.current = false;
        setNotice(event.payload.message ?? "Pendrive gravado com sucesso.");
        setSelectedDeviceId(null);
        void refreshDevices();
      } else if (event.payload.phase === "error") {
        startPending.current = false;
        setNotice(event.payload.message ?? "A gravação foi interrompida.");
      }
    }).then((dispose) => {
      if (disposed) dispose();
      else { unlisten = dispose; setProgressReady(true); }
    }).catch((error) => {
      if (!disposed) setNotice(`Não foi possível acompanhar a gravação: ${readableError(error)}`);
    });

    return () => { disposed = true; unlisten?.(); };
  }, [refreshDevices]);

  useEffect(() => {
    if (!isRunningInTauri() || !isBusy) return;
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    void getCurrentWindow().onCloseRequested((event) => {
      event.preventDefault();
      setNotice("Aguarde a conclusão da gravação antes de fechar o Meraki Flash.");
    }).then((dispose) => { if (disposed) dispose(); else unlisten = dispose; })
      .catch(() => {});
    return () => { disposed = true; unlisten?.(); };
  }, [isBusy]);

  const selectIso = async () => {
    setNotice(null);

    if (!isRunningInTauri()) {
      browserFileInput.current?.click();
      return;
    }

    try {
      const path = await open({
        multiple: false,
        directory: false,
        title: "Selecione uma imagem ISO",
        filters: [{ name: "Imagem de disco", extensions: ["iso"] }],
      });

      if (typeof path === "string") {
        resetProgress();
        setSelectedIso({ path, name: fileNameFromPath(path) });
      }
    } catch (error) {
      setNotice(`Não foi possível abrir o seletor: ${readableError(error)}`);
    }
  };

  const handleBrowserFile = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (file) { resetProgress(); setSelectedIso({ name: file.name, path: file.name }); }
    event.target.value = "";
  };

  const handleStart = async () => {
    if (!isReady || isBusy || startPending.current || !progressReady || !operatingSystem || !selectedIso || !selectedDevice) return;

    let unattendXmlContent: string | null = null;
    if (operatingSystem === "windows" && unattendEnabled) {
      const error = validateUnattendOptions(unattendOptions);
      if (error) {
        setNotice(error);
        return;
      }
      try {
        unattendXmlContent = generateAutounattendXml(unattendOptions);
      } catch (error) {
        setNotice(readableError(error));
        return;
      }
    }

    startPending.current = true;
    activeOperation.current = null;
    setIsConfirming(true);
    try {
    const approved = await confirm(
      `TODOS OS DADOS de ${selectedDevice.name} (${formatBytes(selectedDevice.totalBytes)}, ${selectedDevice.devicePath}) serão apagados.${unattendXmlContent ? "\n\nO perfil de instalação automática será incluído na raiz do pendrive." : ""}\n\nConfirme apenas se este é o pendrive correto.`,
      {
        title: "Confirmar gravação destrutiva",
        kind: "warning",
        okLabel: "Apagar e gravar",
        cancelLabel: "Cancelar",
      },
    );
    if (!approved) { startPending.current = false; return; }

    setNotice(null);
    setProgress({
      phase: "preparing",
      percentage: 0,
      bytesPerSecond: 0,
      etaSeconds: null,
      message: "Aguardando autorização administrativa…",
    });

      await invoke<{ operationId: string }>("start_flash", {
        request: {
          isoPath: selectedIso.path,
          deviceId: selectedDevice.id,
          imageKind: operatingSystem,
          unattendXmlContent,
        },
      });
    } catch (error) {
      startPending.current = false;
      const message = readableError(error);
      setProgress({
        phase: "error",
        percentage: 0,
        bytesPerSecond: 0,
        etaSeconds: null,
        message,
      });
      setNotice(message);
    } finally {
      setIsConfirming(false);
    }
  };

  return (
    <div className="relative min-h-screen overflow-x-hidden bg-[#0B0C10] text-white">
      <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_16%_-8%,rgba(0,240,255,0.10),transparent_31%),radial-gradient(circle_at_88%_20%,rgba(138,43,226,0.12),transparent_32%)]" />
      <div className="pointer-events-none absolute inset-0 opacity-[0.025] [background-image:linear-gradient(rgba(255,255,255,.8)_1px,transparent_1px),linear-gradient(90deg,rgba(255,255,255,.8)_1px,transparent_1px)] [background-size:52px_52px]" />

      <div className="relative mx-auto flex min-h-screen w-full max-w-[1440px] flex-col px-5 py-5 sm:px-8 lg:px-10">
        <header className="flex items-center justify-between border-b border-white/[0.06] pb-5">
          <div className="flex items-center gap-3">
            <MerakiMark />
            <div>
              <div className="flex items-baseline gap-2">
                <h1 className="text-lg font-semibold tracking-[-0.02em]">Meraki Flash</h1>
              </div>
              <p className="mt-0.5 text-xs text-white/40">Crie. Grave. Inicialize.</p>
            </div>
          </div>

        </header>

        <main className="flex flex-1 flex-col py-7 lg:py-9">
          <UpdateNotice />
          <div className="mb-7 grid gap-6 lg:grid-cols-[1fr_auto] lg:items-end">
            <div>
              <p className="mb-2 flex items-center gap-2 text-[11px] font-semibold tracking-[0.2em] text-[#5cefff] uppercase">
                <span className="h-px w-7 bg-[#00F0FF]" />
                Nova mídia inicializável
              </p>
              <h2 className="max-w-2xl text-3xl font-semibold tracking-[-0.04em] sm:text-4xl">
                Seu sistema, pronto para uso
              </h2>
              <p className="mt-2 max-w-2xl text-sm leading-6 text-white/45">
                Escolha a imagem e o pendrive. O Meraki cuida do restante com segurança.
              </p>
            </div>
            <StepRail currentStep={currentStep} selections={stepSelections} />
          </div>

          <div className="grid flex-1 gap-4 min-[1080px]:grid-cols-[1fr_1fr_1.12fr]">
            <section className="meraki-card flex min-h-[244px] flex-col p-5">
              <SectionHeading
                number="01"
                title="Escolha o sistema"
                subtitle="Qual sistema existe na imagem?"
                complete={Boolean(operatingSystem)}
              />

              <div className="os-grid mt-5 grid flex-1 gap-3">
                {osOptions.map((option) => {
                  const Icon = option.icon;
                  const selected = operatingSystem === option.id;
                  return (
                    <button
                      key={option.id}
                      type="button"
                      disabled={isBusy}
                      aria-pressed={selected}
                      onClick={() => { resetProgress(); setOperatingSystem(option.id); }}
                      className={`group relative overflow-hidden rounded-2xl border p-4 text-left transition duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#00F0FF]/70 ${
                        selected
                          ? "border-[#00F0FF]/35 bg-[#00F0FF]/[0.07] shadow-[inset_0_0_28px_rgba(0,240,255,0.03)]"
                          : "border-white/[0.07] bg-white/[0.025] hover:border-white/15 hover:bg-white/[0.045]"
                      }`}
                    >
                      <span
                        className={`mb-7 flex size-10 items-center justify-center rounded-xl bg-gradient-to-br min-[1080px]:mb-4 ${option.accent} shadow-lg shadow-black/20`}
                      >
                        <Icon className="size-5 text-white" strokeWidth={1.8} />
                      </span>
                      <span className="flex items-center justify-between gap-3">
                        <span>
                          <span className="block text-sm font-semibold">{option.title}</span>
                          <span className="mt-1 block text-[11px] leading-4 text-white/38">
                            {option.description}
                          </span>
                        </span>
                        <span
                          className={`flex size-5 shrink-0 items-center justify-center rounded-full border transition ${
                            selected
                              ? "border-[#00F0FF] bg-[#00F0FF] text-[#061014]"
                              : "border-white/15 text-transparent group-hover:border-white/30"
                          }`}
                        >
                          <Check className="size-3" strokeWidth={3} />
                        </span>
                      </span>
                    </button>
                  );
                })}
              </div>
            </section>

            <section className="meraki-card flex min-h-[244px] flex-col p-5">
              <SectionHeading
                number="02"
                title="Selecione a ISO"
                subtitle="Use uma imagem .iso válida"
                complete={Boolean(selectedIso)}
              />

              <button
                type="button"
                disabled={isBusy}
                onClick={() => void selectIso()}
                className={`group mt-5 flex flex-1 flex-col items-center justify-center rounded-2xl border border-dashed px-5 py-7 text-center transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#00F0FF]/70 ${
                  selectedIso
                    ? "border-[#8A2BE2]/35 bg-[#8A2BE2]/[0.055]"
                    : "border-white/10 bg-white/[0.018] hover:border-[#00F0FF]/25 hover:bg-[#00F0FF]/[0.025]"
                }`}
              >
                <span
                  className={`flex size-12 items-center justify-center rounded-2xl border transition ${
                    selectedIso
                      ? "border-[#8A2BE2]/30 bg-[#8A2BE2]/15 text-[#c697ff]"
                      : "border-white/[0.08] bg-white/[0.035] text-white/55 group-hover:border-[#00F0FF]/20 group-hover:text-[#62f3ff]"
                  }`}
                >
                  {selectedIso ? <FileArchive className="size-5" /> : <Upload className="size-5" />}
                </span>
                <span className="mt-4 max-w-full break-all text-sm font-medium">
                  {selectedIso ? selectedIso.name : "Clique para escolher uma ISO"}
                </span>
                <span className="mt-1 max-w-full truncate text-xs text-white/35">
                  {selectedIso ? selectedIso.path : "Windows ou Linux · arquivo único"}
                </span>
                <span className="mt-5 inline-flex items-center gap-2 rounded-xl border border-white/[0.08] bg-white/[0.035] px-3 py-2 text-[11px] font-medium text-white/65">
                  <FolderOpen className="size-3.5" />
                  {selectedIso ? "Trocar arquivo" : "Abrir explorador"}
                </span>
              </button>
              <input
                ref={browserFileInput}
                type="file"
                accept=".iso,application/x-iso9660-image"
                className="hidden"
                onChange={handleBrowserFile}
              />
            </section>

            <section className="meraki-card flex min-h-[244px] flex-col p-5">
              <SectionHeading
                number="03"
                title="Escolha o pendrive"
                subtitle="Somente unidades removíveis"
                complete={Boolean(selectedDevice)}
                action={
                  <button
                    type="button"
                    onClick={() => void refreshDevices()}
                    disabled={isRefreshing || isBusy}
                    aria-label="Atualizar lista de pendrives"
                    className="flex size-8 items-center justify-center rounded-lg border border-white/[0.07] bg-white/[0.03] text-white/45 transition hover:border-white/15 hover:text-white disabled:opacity-50"
                  >
                    <RefreshCw className={`size-3.5 ${isRefreshing ? "animate-spin" : ""}`} />
                  </button>
                }
              />

              <div className="mt-5 flex flex-1 flex-col gap-2.5">
                {isRefreshing ? (
                  <EmptyDeviceState icon={LoaderCircle} title="Procurando pendrives…" spinning />
                ) : devices.length === 0 ? (
                  <EmptyDeviceState
                    icon={Usb}
                    title="Nenhum pendrive encontrado"
                    detail={deviceError ?? "Conecte uma unidade USB e atualize a lista."}
                  />
                ) : (
                  devices.map((device) => {
                    const selected = device.id === selectedDeviceId;
                    return (
                      <button
                        key={device.id}
                        type="button"
                        aria-pressed={selected}
                        disabled={device.readOnly || isBusy}
                        onClick={() => { resetProgress(); setSelectedDeviceId(device.id); }}
                        className={`group flex items-center gap-3 rounded-2xl border p-3.5 text-left transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#00F0FF]/70 disabled:cursor-not-allowed disabled:opacity-45 ${
                          selected
                            ? "border-[#00F0FF]/35 bg-[#00F0FF]/[0.065]"
                            : "border-white/[0.07] bg-white/[0.025] hover:border-white/15 hover:bg-white/[0.045]"
                        }`}
                      >
                        <span
                          className={`flex size-10 shrink-0 items-center justify-center rounded-xl border ${
                            selected
                              ? "border-[#00F0FF]/25 bg-[#00F0FF]/10 text-[#55f4ff]"
                              : "border-white/[0.07] bg-white/[0.035] text-white/45"
                          }`}
                        >
                          <Usb className="size-5" />
                        </span>
                        <span className="min-w-0 flex-1">
                          <span className="flex items-center gap-2">
                            <span className="truncate text-sm font-semibold">{device.name}</span>
                            {device.readOnly && (
                              <LockKeyhole className="size-3.5 shrink-0 text-amber-300" />
                            )}
                          </span>
                          <span className="mt-1 block truncate text-[11px] text-white/38">
                            {formatBytes(device.totalBytes)} · {device.fileSystem || "Sem formato"} · {device.mountPoint || device.devicePath}
                          </span>
                        </span>
                        <span
                          className={`flex size-5 shrink-0 items-center justify-center rounded-full border ${
                            selected
                              ? "border-[#00F0FF] bg-[#00F0FF] text-[#071215]"
                              : "border-white/15 text-transparent"
                          }`}
                        >
                          <Check className="size-3" strokeWidth={3} />
                        </span>
                      </button>
                    );
                  })
                )}
              </div>

              <div className="mt-3 flex items-start gap-2 rounded-xl bg-[#5fe8d3]/[0.055] px-3 py-2.5 text-[10px] leading-4 text-[#9adfd5]/70">
                <ShieldCheck className="mt-0.5 size-3.5 shrink-0" />
                Discos internos são ocultados para reduzir o risco de seleção acidental.
              </div>
            </section>
          </div>

          {showUnattendPanel && (
            <UnattendPanel
              enabled={unattendEnabled}
              onEnabledChange={setUnattendEnabled}
              options={unattendOptions}
              onOptionsChange={setUnattendOptions}
              disabled={isBusy}
            />
          )}

          <section className="meraki-card mt-4 grid gap-5 p-5 lg:grid-cols-[1fr_auto] lg:items-center">
            <div className="min-w-0">
              <div className="mb-3 flex items-center justify-between gap-4">
                <div className="flex items-center gap-3">
                  <span className="flex size-9 items-center justify-center rounded-xl bg-white/[0.035] text-white/50">
                    <HardDrive className="size-4" />
                  </span>
                  <div>
                    <p className="text-xs font-semibold text-white/85">04 · Gravação</p>
                    <p className="mt-0.5 text-[11px] text-white/35">{phaseLabels[progress.phase]}</p>
                  </div>
                </div>
                <span className="font-mono text-sm font-semibold text-[#72f5ff] tabular-nums">
                  {Math.round(progress.percentage)}%
                </span>
              </div>

              <div className="h-2 overflow-hidden rounded-full bg-white/[0.055]">
                <div
                  className={`h-full rounded-full bg-gradient-to-r from-[#00F0FF] via-[#2878ff] to-[#8A2BE2] transition-[width] duration-500 ${isBusy ? "progress-glow" : ""}`}
                  style={{ width: `${Math.min(100, Math.max(0, progress.percentage))}%` }}
                />
              </div>

              <div className="mt-3 flex flex-wrap gap-x-5 gap-y-2 text-[11px] text-white/38">
                <span className="flex items-center gap-1.5">
                  <Gauge className="size-3.5" />
                  {progress.bytesPerSecond > 0 ? `${formatBytes(progress.bytesPerSecond)}/s` : "— MB/s"}
                </span>
                <span className="flex items-center gap-1.5">
                  <Clock3 className="size-3.5" />
                  {progress.etaSeconds === null ? "Tempo restante —" : formatEta(progress.etaSeconds)}
                </span>
                {progress.message && <span className="text-white/55">{progress.message}</span>}
              </div>
            </div>

            <button
              type="button"
              onClick={() => void handleStart()}
              disabled={!isReady || isBusy || !progressReady || isRefreshing}
              className="group relative min-w-[210px] overflow-hidden rounded-2xl bg-gradient-to-r from-[#00F0FF] via-[#2878ff] to-[#8A2BE2] p-px shadow-[0_12px_38px_rgba(36,122,255,0.18)] transition hover:-translate-y-0.5 hover:shadow-[0_14px_42px_rgba(36,122,255,0.28)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#00F0FF]/70 disabled:translate-y-0 disabled:cursor-not-allowed disabled:opacity-35 disabled:shadow-none"
            >
              <span className="flex items-center justify-center gap-2 rounded-[15px] bg-[#101119]/90 px-5 py-3.5 text-sm font-semibold transition group-hover:bg-[#101119]/75">
                {isBusy ? (
                  <LoaderCircle className="size-4 animate-spin" />
                ) : (
                  <Upload className="size-4" />
                )}
                {isBusy
                  ? progress.phase === "preparing"
                    ? "Preparando…"
                    : progress.phase === "analyzing"
                      ? "Analisando…"
                      : progress.phase === "splitting"
                        ? "Dividindo WIM…"
                        : progress.phase === "formatting"
                          ? "Formatando…"
                    : progress.phase === "syncing"
                      ? "Sincronizando…"
                      : progress.phase === "verifying"
                        ? "Verificando…"
                      : progress.phase === "extracting"
                        ? "Extraindo…"
                      : "Gravando…"
                  : "Gravar pendrive"}
                {!isBusy && <ArrowRight className="size-4 transition group-hover:translate-x-0.5" />}
              </span>
            </button>
          </section>

          {notice && (
            <div className="mt-3 flex items-start gap-2.5 rounded-xl border border-[#00F0FF]/10 bg-[#00F0FF]/[0.035] px-4 py-3 text-xs leading-5 text-[#b8f9fd]/75">
              <Info className="mt-0.5 size-4 shrink-0 text-[#61eff8]" />
              <span className="flex-1">{notice}</span>
              <button
                type="button"
                onClick={() => setNotice(null)}
                className="text-white/35 transition hover:text-white"
                aria-label="Fechar aviso"
              >
                ×
              </button>
            </div>
          )}
        </main>
      </div>
    </div>
  );
}

function MerakiMark() {
  return (
    <div className="relative size-11 overflow-hidden rounded-[14px] border border-[#00F0FF]/20 bg-[#12131a] shadow-[0_0_24px_rgba(0,240,255,0.12)]">
      <img src={merakiFlashIcon} alt="" className="size-full object-cover" aria-hidden="true" />
    </div>
  );
}

function SectionHeading({
  number,
  title,
  subtitle,
  complete,
  action,
}: {
  number: string;
  title: string;
  subtitle: string;
  complete: boolean;
  action?: React.ReactNode;
}) {
  return (
    <div className="flex items-start gap-3">
      <span
        className={`mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-lg border text-[10px] font-bold ${
          complete
            ? "border-[#5fe8d3]/25 bg-[#5fe8d3]/10 text-[#72e8d6]"
            : "border-white/[0.08] bg-white/[0.025] text-white/35"
        }`}
      >
        {complete ? <Check className="size-3.5" strokeWidth={2.5} /> : number}
      </span>
      <div className="min-w-0 flex-1">
        <h3 className="text-sm font-semibold tracking-[-0.01em]">{title}</h3>
        <p className="mt-0.5 text-[11px] text-white/35">{subtitle}</p>
      </div>
      {action}
    </div>
  );
}

function StepRail({ currentStep, selections }: { currentStep: number; selections: boolean[] }) {
  return (
    <div className="flex items-center gap-2 rounded-2xl border border-white/[0.06] bg-white/[0.018] px-3 py-2.5">
      {[1, 2, 3, 4].map((step, index) => {
        const complete = selections[index];
        const active = step === currentStep;
        return (
          <div key={step} className="flex items-center gap-2">
            <span
              aria-current={active ? "step" : undefined}
              aria-label={`Etapa ${step}${complete ? " selecionada" : ""}`}
              className={`flex size-6 items-center justify-center rounded-full text-[10px] font-bold transition ${
                complete
                  ? "bg-[#5fe8d3] text-[#071310]"
                  : active
                    ? "border border-[#00F0FF]/50 bg-[#00F0FF]/10 text-[#6df5ff]"
                    : "border border-white/[0.09] text-white/28"
              }`}
            >
              {step}
            </span>
            {index < 3 && (
              <span className={`h-px w-5 ${complete ? "bg-[#5fe8d3]/45" : "bg-white/[0.08]"}`} />
            )}
          </div>
        );
      })}
    </div>
  );
}

function EmptyDeviceState({
  icon: Icon,
  title,
  detail,
  spinning = false,
}: {
  icon: typeof Usb;
  title: string;
  detail?: string;
  spinning?: boolean;
}) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center rounded-2xl border border-dashed border-white/[0.08] bg-white/[0.015] px-5 py-6 text-center">
      <Icon className={`size-6 text-white/25 ${spinning ? "animate-spin" : ""}`} />
      <p className="mt-3 text-xs font-medium text-white/58">{title}</p>
      {detail && <p className="mt-1 max-w-xs text-[10px] leading-4 text-white/28">{detail}</p>}
    </div>
  );
}

function isRunningInTauri() {
  return "__TAURI_INTERNALS__" in window;
}

function fileNameFromPath(path: string) {
  return path.split(/[\\/]/).pop() || path;
}

function readableError(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function formatBytes(bytes: number) {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** index;
  return `${new Intl.NumberFormat("pt-BR", { maximumFractionDigits: index > 2 ? 1 : 0 }).format(value)} ${units[index]}`;
}

function formatEta(seconds: number) {
  if (seconds <= 0) return "Concluído";
  if (seconds < 60) return `${Math.max(1, Math.round(seconds))}s restantes`;
  const rounded = Math.round(seconds);
  const minutes = Math.floor(rounded / 60);
  const remainingSeconds = rounded % 60;
  return `${minutes}min ${remainingSeconds}s restantes`;
}

export default App;
