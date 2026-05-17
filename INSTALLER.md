# Lucy Assistant — Installer Guide

This document covers everything operations teams need to deploy Lucy across an estate: artefact selection, silent installs, GPO / Intune / SCCM wrappers, uninstall, and what the installer actually does on disk + registry.

The Lucy installer ships in three flavors, all produced from a single `cargo tauri build` run against `src-tauri/tauri.conf.json`. They share the same payload — pick by deployment channel.

| Artefact | Size | Channel | Notes |
|---|---|---|---|
| `Lucy Assistant_1.4.0_x64-setup.exe` | 197 MB | Interactive / NSIS | Bilingual selector (en/es/es-Intl), per-user or per-machine at runtime, sidebar+header branded, license page. |
| `Lucy Assistant_1.4.0_x64_en-US.msi` | 197 MB | Enterprise / GPO / Intune | Per-machine only, English UI strings. |
| `Lucy Assistant_1.4.0_x64_es-ES.msi` | 197 MB | Enterprise / GPO / Intune | Per-machine only, Spanish UI strings. |

All three embed the **WebView2 Evergreen Runtime offline installer** (~120 MB of the 197 MB total). This is deliberate — Lucy is a SysAdmin tool that frequently runs on locked-down corp hosts where the WebView2 bootstrapper's download path is blocked at the firewall. With the offline installer baked in, first launch works without internet.

---

## Quick start (interactive)

1. Download `Lucy Assistant_1.4.0_x64-setup.exe`.
2. Double-click. Pick your language (English / Spanish / Spanish International).
3. Choose `Install for me only` (no UAC) or `Install for all users` (requires admin).
4. Accept the GPLv3 license.
5. Optionally tick `Create desktop shortcut`.
6. Done. Lucy launches from the Start Menu under **Lucy Assistant**.

The installer registers Lucy in **Programs & Features** with publisher *Iván Eduardo Luna (@Phenomx64L)* and the homepage link, so the uninstaller is discoverable through Windows' normal channels.

---

## Silent install — interactive .exe

NSIS supports the `/S` switch for silent mode. Combine with `/D=` to override the install directory.

```cmd
:: per-user, default path
"Lucy Assistant_1.4.0_x64-setup.exe" /S

:: per-machine (needs elevation), custom path
"Lucy Assistant_1.4.0_x64-setup.exe" /S /D=C:\Tools\Lucy

:: pin language up-front (skips the selector)
"Lucy Assistant_1.4.0_x64-setup.exe" /S /L=1033   :: 1033 = English (LCID)
"Lucy Assistant_1.4.0_x64-setup.exe" /S /L=3082   :: 3082 = Spanish (Spain)
"Lucy Assistant_1.4.0_x64-setup.exe" /S /L=2058   :: 2058 = Spanish (Mexico, International)
```

Exit codes:
- `0` — success
- `1` — user cancelled (only meaningful in non-silent mode)
- `2` — install failed (check `%TEMP%\Lucy Assistant_1.4.0_x64-install.log`)

---

## Silent install — MSI (recommended for fleet deployment)

MSI is the channel SCCM, Intune, and GPO Software Installation policies expect.

```cmd
:: silent, all users, log to file
msiexec /i "Lucy Assistant_1.4.0_x64_en-US.msi" /qn /norestart ^
  /l*v "C:\Windows\Temp\lucy-install.log"

:: passive UI (progress only, no prompts)
msiexec /i "Lucy Assistant_1.4.0_x64_en-US.msi" /passive /norestart

:: uninstall by product code (see below for how to find it)
msiexec /x "{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}" /qn
```

### Finding the product code post-install

```powershell
Get-WmiObject Win32_Product `
  | Where-Object { $_.Name -like 'Lucy Assistant*' } `
  | Select-Object Name, Version, IdentifyingNumber
```

The `IdentifyingNumber` is the GUID you pass to `msiexec /x`.

### Common MSI properties

| Property | Default | Purpose |
|---|---|---|
| `INSTALLDIR` | `%ProgramFiles%\Lucy Assistant` | Override install root. |
| `ALLUSERS` | `1` | MSI is hardcoded per-machine; do not change. |
| `REBOOT` | `ReallySuppress` (with `/norestart`) | Block automatic reboots. |

```cmd
msiexec /i "Lucy Assistant_1.4.0_x64_en-US.msi" /qn ^
  INSTALLDIR="D:\Programs\Lucy" REBOOT=ReallySuppress
```

---

## Group Policy (GPO) deployment

1. Place the MSI on a UNC share readable by `Domain Computers`: `\\fileserver\deploy\Lucy\Lucy Assistant_1.4.0_x64_en-US.msi`.
2. Open **Group Policy Management** → edit the target GPO.
3. Navigate to **Computer Configuration → Policies → Software Settings → Software Installation**.
4. Right-click → **New → Package** → browse to the UNC path → choose **Assigned**.
5. (Optional) Right-click the package → **Properties → Deployment → Advanced** → enable `Uninstall this application when it falls out of the scope of management`.
6. `gpupdate /force` on a target host or wait for the next refresh cycle. Lucy installs on next reboot.

Use the en-US MSI for global rollouts; ship the es-ES MSI to OUs whose users are Spanish-speaking.

---

## Intune (Windows Autopilot / MEM)

Intune does **not** accept .msi directly for modern Win32 deployments — wrap into `.intunewin`:

```powershell
# Run once on your packaging workstation
# Tool: IntuneWinAppUtil.exe (download from Microsoft)

.\IntuneWinAppUtil.exe `
  -c "C:\packaging\source" `       # folder containing the .msi
  -s "Lucy Assistant_1.4.0_x64_en-US.msi" `
  -o "C:\packaging\output"
```

In the Intune portal, create a new **Windows app (Win32)**:

- Install command:
  ```
  msiexec /i "Lucy Assistant_1.4.0_x64_en-US.msi" /qn /norestart
  ```
- Uninstall command:
  ```
  msiexec /x "{product-code-guid}" /qn
  ```
- Detection: **Registry** rule — key `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{product-code-guid}`, value `DisplayVersion` exists and equals `1.4.0`.
- Requirements: x64, Windows 10 1809+ (WebView2 minimum).
- Return codes: `0` success, `1707` success (legacy), `3010` soft reboot.

---

## SCCM / MECM

Create a **Standard application**:

- Content: folder containing the .msi.
- Deployment type: **Windows Installer (Native)**, point at the .msi — SCCM auto-detects the product code and version.
- Install behaviour: **Install for system**, whether or not a user is logged on.
- User experience: **Hidden**, **Allow user to view and interact: No**.
- Return codes: leave defaults (`0` success, `3010` soft reboot, `1641` hard reboot).

---

## What the installer does on disk + registry

### Files

```
C:\Program Files\Lucy Assistant\                    (per-machine — MSI / NSIS /D)
└── lucy-svelte.exe                                  ~30 MB main binary
└── WebView2Loader.dll
└── resources\                                       Tauri-bundled SvelteKit build
└── (WebView2 runtime if not already present)        ~120 MB

%LOCALAPPDATA%\Programs\Lucy Assistant\              (per-user mode)
└── (same layout, no admin needed)

%APPDATA%\com.lucy.dev\                              (user data — created on first launch)
├── lucy.db                                          SQLite: tabs, agent memories, runbooks
├── lucy.db-wal / lucy.db-shm                        SQLite WAL mode
├── secrets.json                                     encrypted MCP secrets
└── logs\                                            rolling structured logs
```

### Registry

```
HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{product-code}
    DisplayName       = "Lucy Assistant"
    DisplayVersion    = "1.4.0"
    Publisher         = "Iván Eduardo Luna (@Phenomx64L)"
    URLInfoAbout      = "https://github.com/Phenomx64L/LucyAI"
    InstallLocation   = "C:\Program Files\Lucy Assistant"
    UninstallString   = "msiexec /x {product-code}"      (MSI)
                        "C:\Program Files\Lucy Assistant\uninstall.exe"   (NSIS)
    EstimatedSize     = ~200000  (KB)
```

The per-user NSIS install writes to `HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\...` instead — no admin rights needed and no machine-wide footprint.

### Shortcuts

- Start Menu: `%ProgramData%\Microsoft\Windows\Start Menu\Programs\Lucy Assistant\Lucy Assistant.lnk` (per-machine) or `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Lucy Assistant\Lucy Assistant.lnk` (per-user).
- Desktop: optional checkbox in NSIS, opt-in only.

---

## Uninstall

### Interactive
**Programs & Features** → `Lucy Assistant` → **Uninstall**.

### Silent
```cmd
:: MSI
msiexec /x "{product-code-guid}" /qn /norestart

:: NSIS
"%ProgramFiles%\Lucy Assistant\uninstall.exe" /S
```

### User data is NOT removed by uninstall

By design — operators reinstalling won't lose tab history, memories, or runbooks. To wipe user data:

```powershell
Remove-Item -Recurse -Force "$env:APPDATA\com.lucy.dev"
```

---

## Verifying an install programmatically

```powershell
# Returns the installed version, or $null if absent
function Get-LucyVersion {
    Get-ItemProperty `
      'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
      'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*',
      'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*' `
      -ErrorAction SilentlyContinue `
    | Where-Object { $_.DisplayName -eq 'Lucy Assistant' } `
    | Select-Object -ExpandProperty DisplayVersion -First 1
}

Get-LucyVersion   # → 1.4.0  (or $null)
```

Use this in your deployment detection scripts (Intune, SCCM, Ansible win_reg lookup, etc.).

---

## Known limitations

- **Code signing**: the installer is **unsigned**. SmartScreen will show *"Windows protected your PC — Unknown publisher"* on first run; click **More info → Run anyway**. To suppress this for fleet deployments, sign the .exe and .msi with an OV/EV code-signing certificate (Sectigo / DigiCert / SSL.com) and set `bundle.windows.certificateThumbprint` in `tauri.conf.json`.
- **WebView2 runtime**: bundled, so 197 MB. If you control the fleet and know WebView2 is preinstalled (Win11, recent Win10 + Edge), you can rebuild with `webviewInstallMode.type = "embedBootstrapper"` to drop ~120 MB.
- **No auto-update yet**: ship new versions via your existing MDM / GPO channel for now. The `tauri-plugin-updater` integration is on the roadmap.
- **MSI is per-machine only**: WiX does not support per-user MSI in our config. Use the NSIS .exe with `/CURRENTUSER` for that mode.

---

## Building installers from source

```bash
# Prerequisites: Rust toolchain, Node 20+, WiX 3.x on PATH, NSIS bundled by Tauri.
cd lucy-svelte
npm install
npx tauri build
```

Output lands in `src-tauri/target/release/bundle/{nsis,msi}/`. Build time on a modern Ryzen 5950X is ~2 min for the Rust release compile + ~30 s for bundling.

To rebrand the NSIS sidebar/header BMPs:

```bash
python src-tauri/icons/source/gen_installer_banners.py
```

The script regenerates `installer-sidebar.bmp` (164×314) and `installer-header.bmp` (150×57) from `icons/source/lucy-icon-1024.png`. Tweak colors, text, or fonts at the top of the script — Pillow-only, no external deps.

---

## Support

Issues, deployment questions, or signing-cert recommendations welcome at:
https://github.com/Phenomx64L/LucyAI/issues
