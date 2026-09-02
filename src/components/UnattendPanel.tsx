import {
  BotOff,
  Check,
  Cpu,
  Gauge,
  SearchX,
  ShieldCheck,
  UserRound,
  WandSparkles,
  type LucideIcon,
} from "lucide-react";
import {
  type UnattendOptions,
  validateUnattendOptions,
  validateWindowsUsername,
} from "../lib/unattend";

interface UnattendPanelProps {
  enabled: boolean;
  onEnabledChange: (enabled: boolean) => void;
  options: UnattendOptions;
  onOptionsChange: (options: UnattendOptions) => void;
  disabled?: boolean;
}

interface OptimizationToggleProps {
  checked: boolean;
  title: string;
  description: string;
  icon: LucideIcon;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  destructive?: boolean;
}

export function UnattendPanel({
  enabled,
  onEnabledChange,
  options,
  onOptionsChange,
  disabled = false,
}: UnattendPanelProps) {
  const validationError = enabled ? validateUnattendOptions(options) : null;
  const usernameError =
    enabled && options.createLocalAccount ? validateWindowsUsername(options.username) : null;

  const update = <K extends keyof UnattendOptions>(key: K, value: UnattendOptions[K]) => {
    onOptionsChange({ ...options, [key]: value });
  };

  return (
    <section className="mt-4 overflow-hidden rounded-2xl border border-[#8cbe80]/15 bg-[#171E15] shadow-[0_20px_60px_rgba(0,0,0,.18)]">
      <label className="flex cursor-pointer items-center gap-4 p-5 transition hover:bg-white/[0.025] sm:p-6">
        <input
          type="checkbox"
          checked={enabled}
          onChange={(event) => onEnabledChange(event.target.checked)}
          disabled={disabled}
          className="peer sr-only"
        />
        <span className="flex size-11 shrink-0 items-center justify-center rounded-2xl border border-[#9dd68f]/20 bg-[#9dd68f]/[0.08] text-[#b8f2aa] peer-focus-visible:ring-2 peer-focus-visible:ring-[#00F0FF]/70">
          <WandSparkles className="size-5" />
        </span>
        <span className="min-w-0 flex-1">
          <span className="flex flex-wrap items-center gap-2">
            <span className="text-sm font-semibold text-white">Aplicar Otimizações (Unattended)</span>
            <span className="rounded-full border border-[#9dd68f]/15 bg-[#9dd68f]/[0.07] px-2 py-0.5 text-[9px] font-bold tracking-[0.14em] text-[#b8f2aa] uppercase">
              Windows x64
            </span>
          </span>
          <span className="mt-1 block text-xs leading-5 text-white/40">
            Gera o autounattend.xml e adiciona o arquivo à raiz do pendrive.
          </span>
        </span>
        <span
          aria-hidden="true"
          className={`flex size-6 shrink-0 items-center justify-center rounded-lg border transition ${
            enabled
              ? "border-[#9dd68f] bg-[#9dd68f] text-[#11180f]"
              : "border-white/15 bg-black/10 text-transparent"
          }`}
        >
          <Check className="size-3.5" strokeWidth={3} />
        </span>
      </label>

      {enabled && (
        <div className="border-t border-[#9dd68f]/10 px-5 py-5 sm:px-6 sm:py-6">
          <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
            <OptimizationToggle
              checked={options.bypassWindows11Requirements}
              onChange={(checked) => update("bypassWindows11Requirements", checked)}
              icon={Cpu}
              title="Ignorar requisitos do Windows 11"
              description="Contorna as verificações de TPM 2.0, Secure Boot e memória RAM."
              disabled={disabled}
            />
            <div>
              <OptimizationToggle
                checked={options.createLocalAccount}
                onChange={(checked) => update("createLocalAccount", checked)}
                icon={UserRound}
                title="Criar conta local offline"
                description="Cria uma conta Administrador sem senha inicial."
                disabled={disabled}
              />
              {options.createLocalAccount && (
                <div className="mt-2 rounded-2xl border border-white/[0.07] bg-black/[0.12] p-4">
                  <label htmlFor="unattend-username" className="text-[10px] font-semibold tracking-[0.12em] text-white/45 uppercase">
                    Nome do usuário
                  </label>
                  <input
                    id="unattend-username"
                    type="text"
                    value={options.username}
                    maxLength={20}
                    autoComplete="off"
                    spellCheck={false}
                    disabled={disabled}
                    onChange={(event) => update("username", event.target.value)}
                    placeholder="Ex.: Meraki"
                    aria-invalid={Boolean(usernameError)}
                    aria-describedby={usernameError ? "unattend-username-error" : undefined}
                    className="mt-2 w-full rounded-xl border border-white/10 bg-[#0B0C10]/65 px-3 py-2.5 text-sm text-white outline-none transition placeholder:text-white/20 focus:border-[#9dd68f]/45 focus:ring-2 focus:ring-[#9dd68f]/10 disabled:opacity-50"
                  />
                  {usernameError ? (
                    <p id="unattend-username-error" className="mt-2 text-[10px] leading-4 text-amber-300/85">
                      {usernameError}
                    </p>
                  ) : (
                    <p className="mt-2 text-[10px] leading-4 text-white/30">
                      Defina uma senha segura no primeiro acesso.
                    </p>
                  )}
                </div>
              )}
            </div>
            <OptimizationToggle
              checked={options.skipPrivacyQuestions}
              onChange={(checked) => update("skipPrivacyQuestions", checked)}
              icon={ShieldCheck}
              title="Pular perguntas de privacidade"
              description="Reduz telemetria, localização, anúncios e experiências personalizadas."
              disabled={disabled}
            />
            <OptimizationToggle
              checked={options.disableEdgeBackground}
              onChange={(checked) => update("disableEdgeBackground", checked)}
              icon={Gauge}
              title="Desativar Edge em segundo plano"
              description="Desabilita o modo em segundo plano e o Startup Boost do Edge."
              disabled={disabled}
            />
            <OptimizationToggle
              checked={options.hideTaskbarSearch}
              onChange={(checked) => update("hideTaskbarSearch", checked)}
              icon={SearchX}
              title="Ocultar pesquisa da barra de tarefas"
              description="Remove o ícone ou caixa de pesquisa ao lado do botão Iniciar."
              disabled={disabled}
            />
            <OptimizationToggle
              checked={options.removeCopilot}
              onChange={(checked) => update("removeCopilot", checked)}
              icon={BotOff}
              title="Remover Copilot"
              description="Remove os pacotes conhecidos e aplica as políticas de ocultação."
              disabled={disabled}
              destructive
            />
          </div>

          <div className="mt-4 flex flex-col gap-2 rounded-xl border border-[#9dd68f]/10 bg-[#9dd68f]/[0.035] px-4 py-3 text-[11px] leading-5 text-white/42 sm:flex-row sm:items-center sm:justify-between">
            <span>As opções são executadas pelo Instalador do Windows durante a instalação.</span>
            <span className={validationError ? "text-amber-300/90" : "text-[#b8f2aa]/75"}>
              {validationError ?? "Configuração válida e pronta para gerar."}
            </span>
          </div>
        </div>
      )}
    </section>
  );
}

function OptimizationToggle({
  checked,
  title,
  description,
  icon: Icon,
  onChange,
  disabled = false,
  destructive = false,
}: OptimizationToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`flex w-full items-start gap-3 rounded-2xl border p-4 text-left transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#00F0FF]/70 disabled:cursor-not-allowed disabled:opacity-50 ${
        checked
          ? destructive
            ? "border-[#d096ff]/25 bg-[#8A2BE2]/[0.07]"
            : "border-[#9dd68f]/25 bg-[#9dd68f]/[0.055]"
          : "border-white/[0.07] bg-black/[0.12] hover:border-white/15 hover:bg-white/[0.025]"
      }`}
    >
      <span
        className={`flex size-9 shrink-0 items-center justify-center rounded-xl border ${
          checked
            ? destructive
              ? "border-[#d096ff]/25 bg-[#8A2BE2]/15 text-[#d9b4ff]"
              : "border-[#9dd68f]/20 bg-[#9dd68f]/10 text-[#b8f2aa]"
            : "border-white/[0.07] bg-white/[0.03] text-white/35"
        }`}
      >
        <Icon className="size-4" />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block text-xs font-semibold text-white/85">{title}</span>
        <span className="mt-1 block text-[10px] leading-4 text-white/35">{description}</span>
      </span>
      <span
        aria-hidden="true"
        className={`mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full border transition ${
          checked
            ? destructive
              ? "border-[#c689ff] bg-[#c689ff] text-[#160a20]"
              : "border-[#9dd68f] bg-[#9dd68f] text-[#11180f]"
            : "border-white/15 text-transparent"
        }`}
      >
        <Check className="size-3" strokeWidth={3} />
      </span>
    </button>
  );
}
