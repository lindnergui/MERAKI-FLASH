export const RELEASES_URL = "https://github.com/lindnergui/MERAKI-FLASH/releases/latest";
export const UPDATE_PREFERENCE = "meraki-flash.check-updates";

export function isNewerRelease(tag: string, current: string): boolean {
  const stable = /^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
  const latest = stable.exec(tag);
  const installed = /^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.exec(current);
  if (!latest || !installed) return false;
  for (let index = 1; index <= 3; index++) {
    const difference = BigInt(latest[index]) - BigInt(installed[index]);
    if (difference !== 0n) return difference > 0n;
  }
  return Boolean(installed[4]);
}

export async function latestUpdate(current: string, signal: AbortSignal): Promise<string | null> {
  const response = await fetch("https://api.github.com/repos/lindnergui/MERAKI-FLASH/releases/latest", {
    signal, headers: { Accept: "application/vnd.github+json" },
    credentials: "omit", referrerPolicy: "no-referrer",
  });
  if (!response.ok) return null;
  const release = await response.json();
  return !release.draft && !release.prerelease && typeof release.tag_name === "string"
    && isNewerRelease(release.tag_name, current) ? release.tag_name : null;
}
