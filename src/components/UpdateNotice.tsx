import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { latestUpdate, UPDATE_PREFERENCE } from "../lib/updates";

export function UpdateNotice() {
  const [version, setVersion] = useState<string | null>(null);
  const [enabled, setEnabled] = useState(() => {
    try { return localStorage.getItem(UPDATE_PREFERENCE) !== "off"; } catch { return true; }
  });
  const [openError, setOpenError] = useState(false);
  useEffect(() => {
    if (!enabled || !("__TAURI_INTERNALS__" in window)) return;
    const controller = new AbortController();
    const timeout = window.setTimeout(() => controller.abort(), 5000);
    void getVersion().then((current) => latestUpdate(current, controller.signal))
      .then((latest) => { if (!controller.signal.aborted) setVersion(latest); })
      .catch(() => {}) // Sem rede ou limite do GitHub: o aplicativo continua normalmente.
      .finally(() => window.clearTimeout(timeout));
    return () => { controller.abort(); window.clearTimeout(timeout); };
  }, [enabled]);

  return <>
    {version && <div role="status" className="mb-5 flex flex-wrap items-center gap-3 rounded-xl border border-cyan-300/20 bg-cyan-300/5 px-4 py-3 text-xs text-cyan-100">
      <span className="flex-1">Meraki Flash {version} disponível. Atualize quando quiser.</span>
      <button type="button" className="underline" onClick={() => {
        void invoke("open_releases_page").catch(() => setOpenError(true));
      }}>Ver atualização</button>
      <button type="button" aria-label="Fechar aviso de atualização" onClick={() => setVersion(null)}>Agora não</button>
      {openError && <span className="w-full break-all">Abra github.com/lindnergui/MERAKI-FLASH/releases/latest no navegador.</span>}
    </div>}
    <label className="mb-4 flex items-center gap-2 self-end text-[11px] text-white/50">
      <input type="checkbox" checked={enabled} onChange={(event) => {
        const checked = event.target.checked;
        setEnabled(checked);
        if (!checked) setVersion(null);
        try { localStorage.setItem(UPDATE_PREFERENCE, checked ? "on" : "off"); } catch { /* Preferência vale nesta sessão. */ }
      }} /> Avisar sobre novas versões ao abrir
    </label>
  </>;
}
