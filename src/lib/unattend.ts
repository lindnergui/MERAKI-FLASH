export interface UnattendOptions {
  bypassWindows11Requirements: boolean;
  createLocalAccount: boolean;
  username: string;
  skipPrivacyQuestions: boolean;
  disableEdgeBackground: boolean;
  hideTaskbarSearch: boolean;
  removeCopilot: boolean;
}

export const DEFAULT_UNATTEND_OPTIONS: UnattendOptions = {
  bypassWindows11Requirements: true,
  createLocalAccount: false,
  username: "",
  skipPrivacyQuestions: true,
  disableEdgeBackground: false,
  hideTaskbarSearch: true,
  removeCopilot: false,
};

const RESERVED_WINDOWS_NAMES = new Set([
  "administrator",
  "guest",
  "defaultaccount",
  "wdagutilityaccount",
  "con",
  "prn",
  "aux",
  "nul",
  ...Array.from({ length: 9 }, (_, index) => `com${index + 1}`),
  ...Array.from({ length: 9 }, (_, index) => `lpt${index + 1}`),
]);

export function validateWindowsUsername(value: string): string | null {
  const username = value.trim();
  if (!username) return "Informe o nome da conta local.";
  if (username.length > 20) return "Use no máximo 20 caracteres no nome da conta.";
  if (/[/\\"\[\]:;|=,+*?<>@\u0000-\u001f]/u.test(username)) {
    return "O nome contém um caractere que o Windows não aceita.";
  }
  if (username.endsWith(".")) return "O nome da conta não pode terminar com ponto.";
  if (RESERVED_WINDOWS_NAMES.has(username.toLocaleLowerCase("en-US"))) {
    return "Esse nome é reservado pelo Windows. Escolha outro.";
  }
  return null;
}

export function validateUnattendOptions(options: UnattendOptions): string | null {
  const hasSelection =
    options.bypassWindows11Requirements ||
    options.createLocalAccount ||
    options.skipPrivacyQuestions ||
    options.disableEdgeBackground ||
    options.hideTaskbarSearch ||
    options.removeCopilot;

  if (!hasSelection) return "Selecione ao menos uma otimização.";
  return options.createLocalAccount ? validateWindowsUsername(options.username) : null;
}

export function generateAutounattendXml(options: UnattendOptions): string {
  const validationError = validateUnattendOptions(options);
  if (validationError) throw new Error(validationError);

  const windowsPeCommands: string[] = [];
  const specializeCommands: string[] = [];
  const firstLogonCommands: string[] = [];

  if (options.bypassWindows11Requirements) {
    windowsPeCommands.push(
      "reg.exe add HKLM\\SYSTEM\\Setup\\LabConfig /v BypassTPMCheck /t REG_DWORD /d 1 /f",
      "reg.exe add HKLM\\SYSTEM\\Setup\\LabConfig /v BypassSecureBootCheck /t REG_DWORD /d 1 /f",
      "reg.exe add HKLM\\SYSTEM\\Setup\\LabConfig /v BypassRAMCheck /t REG_DWORD /d 1 /f",
    );
  }

  if (options.skipPrivacyQuestions) {
    specializeCommands.push(
      "reg.exe add HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\OOBE /v DisablePrivacyExperience /t REG_DWORD /d 1 /f",
      "reg.exe add HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\DataCollection /v AllowTelemetry /t REG_DWORD /d 0 /f",
      "reg.exe add HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\AdvertisingInfo /v DisabledByGroupPolicy /t REG_DWORD /d 1 /f",
      "reg.exe add HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\AppPrivacy /v LetAppsAccessLocation /t REG_DWORD /d 2 /f",
      "reg.exe add HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\LocationAndSensors /v DisableLocation /t REG_DWORD /d 1 /f",
      "reg.exe add HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\CloudContent /v DisableTailoredExperiencesWithDiagnosticData /t REG_DWORD /d 1 /f",
    );
  }

  if (options.disableEdgeBackground) {
    specializeCommands.push(
      "reg.exe add HKLM\\SOFTWARE\\Policies\\Microsoft\\Edge /v BackgroundModeEnabled /t REG_DWORD /d 0 /f",
      "reg.exe add HKLM\\SOFTWARE\\Policies\\Microsoft\\Edge /v StartupBoostEnabled /t REG_DWORD /d 0 /f",
    );
  }

  if (options.removeCopilot) {
    specializeCommands.push(
      "powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -Command \"Get-AppxProvisionedPackage -Online | Where-Object DisplayName -EQ 'Microsoft.Copilot' | Remove-AppxProvisionedPackage -Online -AllUsers -ErrorAction SilentlyContinue\"",
    );
    firstLogonCommands.push(
      "reg.exe add HKCU\\Software\\Policies\\Microsoft\\Windows\\WindowsCopilot /v TurnOffWindowsCopilot /t REG_DWORD /d 1 /f",
      "reg.exe add HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced /v ShowCopilotButton /t REG_DWORD /d 0 /f",
      "powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -Command \"Get-AppxPackage -Name 'Microsoft.Copilot','Microsoft.Windows.Ai.Copilot.Provider' | Remove-AppxPackage -ErrorAction SilentlyContinue\"",
    );
  }

  if (options.hideTaskbarSearch) {
    firstLogonCommands.push(
      "reg.exe add HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Search /v SearchboxTaskbarMode /t REG_DWORD /d 0 /f",
      "reg.exe add HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Search /v SearchboxTaskbarModeCache /t REG_DWORD /d 0 /f",
    );
  }

  const settings: string[] = [];
  if (windowsPeCommands.length) {
    settings.push(
      renderSettings(
        "windowsPE",
        renderComponent("Microsoft-Windows-Setup", renderRunSynchronous(windowsPeCommands)),
      ),
    );
  }
  if (specializeCommands.length) {
    settings.push(
      renderSettings(
        "specialize",
        renderComponent(
          "Microsoft-Windows-Deployment",
          renderRunSynchronous(specializeCommands),
        ),
      ),
    );
  }

  const shellSetup: string[] = [];
  if (options.createLocalAccount || options.skipPrivacyQuestions) {
    shellSetup.push(renderOobe(options));
  }
  if (options.createLocalAccount) {
    shellSetup.push(renderLocalAccount(options.username.trim()));
  }
  if (firstLogonCommands.length) {
    shellSetup.push(renderFirstLogonCommands(firstLogonCommands));
  }
  if (shellSetup.length) {
    settings.push(
      renderSettings(
        "oobeSystem",
        renderComponent("Microsoft-Windows-Shell-Setup", shellSetup.join("\n")),
      ),
    );
  }

  return [
    '<?xml version="1.0" encoding="utf-8"?>',
    '<unattend xmlns="urn:schemas-microsoft-com:unattend" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State">',
    ...settings,
    "</unattend>",
    "",
  ].join("\n");
}

function renderSettings(pass: string, component: string): string {
  return [`  <settings pass="${pass}">`, component, "  </settings>"].join("\n");
}

function renderComponent(name: string, content: string): string {
  return [
    `    <component name="${name}" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS">`,
    content,
    "    </component>",
  ].join("\n");
}

function renderRunSynchronous(commands: string[]): string {
  return [
    "      <RunSynchronous>",
    ...commands.flatMap((command, index) => [
      '        <RunSynchronousCommand wcm:action="add">',
      `          <Order>${index + 1}</Order>`,
      `          <Path>${escapeXml(command)}</Path>`,
      "        </RunSynchronousCommand>",
    ]),
    "      </RunSynchronous>",
  ].join("\n");
}

function renderOobe(options: UnattendOptions): string {
  const values: string[] = [];
  if (options.createLocalAccount) values.push("        <HideOnlineAccountScreens>true</HideOnlineAccountScreens>");
  if (options.skipPrivacyQuestions) {
    values.push(
      "        <HideEULAPage>true</HideEULAPage>",
      "        <HideOEMRegistrationScreen>true</HideOEMRegistrationScreen>",
      "        <HideWirelessSetupInOOBE>true</HideWirelessSetupInOOBE>",
      "        <ProtectYourPC>3</ProtectYourPC>",
    );
  }
  return ["      <OOBE>", ...values, "      </OOBE>"].join("\n");
}

function renderLocalAccount(username: string): string {
  const escapedUsername = escapeXml(username);
  return [
    "      <UserAccounts>",
    "        <LocalAccounts>",
    '          <LocalAccount wcm:action="add">',
    "            <Password>",
    "              <Value></Value>",
    "              <PlainText>true</PlainText>",
    "            </Password>",
    "            <Description>Conta local criada pelo Meraki Flash</Description>",
    `            <DisplayName>${escapedUsername}</DisplayName>`,
    "            <Group>Administrators</Group>",
    `            <Name>${escapedUsername}</Name>`,
    "          </LocalAccount>",
    "        </LocalAccounts>",
    "      </UserAccounts>",
  ].join("\n");
}

function renderFirstLogonCommands(commands: string[]): string {
  return [
    "      <FirstLogonCommands>",
    ...commands.flatMap((command, index) => [
      '        <SynchronousCommand wcm:action="add">',
      `          <Order>${index + 1}</Order>`,
      `          <CommandLine>${escapeXml(command)}</CommandLine>`,
      "          <RequiresUserInput>false</RequiresUserInput>",
      "        </SynchronousCommand>",
    ]),
    "      </FirstLogonCommands>",
  ].join("\n");
}

function escapeXml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&apos;");
}
