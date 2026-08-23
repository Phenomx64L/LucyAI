; ── lucy.nsi — instalador del shell NATIVO de Lucy ──────────────────────────
;
; QUÉ EMPAQUETA, y conviene decirlo porque hay dos Lucys. Esto empaqueta
; `lucy-egui.exe`: el shell nativo, sin WebView2, sin Tauri. Son 19,6 MB de un
; solo fichero contra los 213 MB del instalador de la versión Tauri, y toda la
; diferencia es el motor de navegador que este binario NO enlaza — que es el
; objetivo entero de la migración.
;
; POR QUÉ NSIS Y NO SOLO MSI. Se pidió un instalador con opción de idioma. Un
; MSI es de UN idioma por diseño: el número de idioma está horneado en el
; paquete, y ofrecer un desplegable dentro exige transformaciones .mst más un
; bootstrapper. NSIS sí tiene selector nativo, y es este.
; El MSI se genera aparte, uno por idioma, que es lo que además quieren los
; despliegues por directiva de grupo.
;
; SE INSTALA AL LADO DE LA VERSIÓN TAURI, no encima. Dos motivos: el shell
; nativo todavía no crea el esquema de la base —lo crea la app Tauri, y sin ella
; arranca sin memoria— y mientras eso siga así, desinstalar la vieja rompería la
; nueva. Comparten `%APPDATA%\com.lucy.dev`, así que ven los mismos datos.

Unicode true
SetCompressor /SOLID lzma

!include "MUI2.nsh"
!include "FileFunc.nsh"

!define NOMBRE      "Lucy"
!define VERSION     "2.0.1"
!define EDITOR      "Iván Eduardo Luna"
!define EXE         "lucy-egui.exe"
!define CLAVE_REG   "Software\Microsoft\Windows\CurrentVersion\Uninstall\LucyNative"

Name          "${NOMBRE} ${VERSION}"
OutFile       "..\dist\Lucy_${VERSION}_x64-setup.exe"
InstallDir    "$PROGRAMFILES64\Lucy"
InstallDirRegKey HKLM "Software\Lucy" "InstallDir"
RequestExecutionLevel admin

VIProductVersion "2.0.1.0"
VIAddVersionKey "ProductName"     "${NOMBRE}"
VIAddVersionKey "FileDescription" "Instalador de Lucy — asistente de administración de sistemas"
VIAddVersionKey "FileVersion"     "${VERSION}"
VIAddVersionKey "ProductVersion"  "${VERSION}"
VIAddVersionKey "LegalCopyright"  "Copyright © 2026 ${EDITOR}. GPLv3."
VIAddVersionKey "CompanyName"     "${EDITOR}"

!define MUI_ICON   "..\..\lucy-svelte\src-tauri\icons\icon.ico"
!define MUI_UNICON "..\..\lucy-svelte\src-tauri\icons\icon.ico"
!define MUI_HEADERIMAGE
!define MUI_HEADERIMAGE_BITMAP "..\..\lucy-svelte\src-tauri\icons\installer-header.bmp"
!define MUI_WELCOMEFINISHPAGE_BITMAP "..\..\lucy-svelte\src-tauri\icons\installer-sidebar.bmp"
!define MUI_ABORTWARNING

; EL SELECTOR SALE SIEMPRE, no solo cuando Windows está en un idioma que no
; conocemos. Es lo que se pidió: poder elegir. Sin esta línea, NSIS elige por su
; cuenta el que case con el sistema y no pregunta nada.
!define MUI_LANGDLL_ALWAYSSHOW
!define MUI_LANGDLL_REGISTRY_ROOT      "HKLM"
!define MUI_LANGDLL_REGISTRY_KEY       "Software\Lucy"
!define MUI_LANGDLL_REGISTRY_VALUENAME "Installer Language"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\..\lucy-svelte\LICENSE"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

; El orden importa: el primero es el que se preselecciona si Windows está en uno
; que no está en la lista.
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "Spanish"
!insertmacro MUI_LANGUAGE "SpanishInternational"
!insertmacro MUI_LANGUAGE "Portuguese"
!insertmacro MUI_LANGUAGE "French"
!insertmacro MUI_LANGUAGE "German"

Function .onInit
    !insertmacro MUI_LANGDLL_DISPLAY
FunctionEnd

Function un.onInit
    !insertmacro MUI_UNGETLANGUAGE
FunctionEnd

Section "Lucy" SecPrincipal
    SectionIn RO
    SetOutPath "$INSTDIR"
    File "..\..\lucy-native-proto\target\release\${EXE}"
    File "..\..\lucy-svelte\LICENSE"

    ; EL IDIOMA ELEGIDO SE LE PASA A LUCY. Sin esto, el selector solo cambiaría
    ; el idioma del instalador — cinco minutos de vida— y la aplicación
    ; arrancaría igual en español. Lucy lo lee al primer arranque, solo si el
    ; operador no ha elegido ya uno.
    Call GuardaIdioma

    WriteRegStr HKLM "Software\Lucy" "InstallDir" "$INSTDIR"
    WriteRegStr HKLM "Software\Lucy" "Version"    "${VERSION}"

    CreateDirectory "$SMPROGRAMS\Lucy"
    CreateShortcut  "$SMPROGRAMS\Lucy\Lucy.lnk" "$INSTDIR\${EXE}"
    CreateShortcut  "$DESKTOP\Lucy.lnk"         "$INSTDIR\${EXE}"

    WriteUninstaller "$INSTDIR\uninstall.exe"
    WriteRegStr   HKLM "${CLAVE_REG}" "DisplayName"     "${NOMBRE} ${VERSION}"
    WriteRegStr   HKLM "${CLAVE_REG}" "DisplayVersion"  "${VERSION}"
    WriteRegStr   HKLM "${CLAVE_REG}" "Publisher"       "${EDITOR}"
    WriteRegStr   HKLM "${CLAVE_REG}" "DisplayIcon"     "$INSTDIR\${EXE}"
    WriteRegStr   HKLM "${CLAVE_REG}" "UninstallString" "$INSTDIR\uninstall.exe"
    WriteRegDWORD HKLM "${CLAVE_REG}" "NoModify" 1
    WriteRegDWORD HKLM "${CLAVE_REG}" "NoRepair" 1
    ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
    IntFmt $0 "0x%08X" $0
    WriteRegDWORD HKLM "${CLAVE_REG}" "EstimatedSize" "$0"
SectionEnd

; El código de idioma que entiende Lucy, a partir del que eligió el instalador.
; Se escribe en HKCU porque es una preferencia DEL USUARIO que instala, no de la
; máquina — y porque ahí es donde la va a buscar la aplicación.
Function GuardaIdioma
    StrCpy $R0 "es"
    ${If} $LANGUAGE == 1033
        StrCpy $R0 "en"
    ${ElseIf} $LANGUAGE == 1034
        StrCpy $R0 "es"
    ${ElseIf} $LANGUAGE == 3082
        StrCpy $R0 "es"
    ${ElseIf} $LANGUAGE == 2070
        StrCpy $R0 "pt"
    ${ElseIf} $LANGUAGE == 1036
        StrCpy $R0 "fr"
    ${ElseIf} $LANGUAGE == 1031
        StrCpy $R0 "de"
    ${EndIf}
    WriteRegStr HKCU "Software\Lucy" "Language" "$R0"
FunctionEnd

Section "Uninstall"
    Delete "$INSTDIR\${EXE}"
    Delete "$INSTDIR\LICENSE"
    Delete "$INSTDIR\uninstall.exe"
    RMDir  "$INSTDIR"
    Delete "$SMPROGRAMS\Lucy\Lucy.lnk"
    RMDir  "$SMPROGRAMS\Lucy"
    Delete "$DESKTOP\Lucy.lnk"
    DeleteRegKey HKLM "${CLAVE_REG}"
    DeleteRegKey HKLM "Software\Lucy"
    ; NO se toca `%APPDATA%\com.lucy.dev`: ahí viven las memorias, los equipos
    ; dados de alta y el historial. Un desinstalador que se lleva los datos por
    ; su cuenta convierte una prueba en una pérdida.
SectionEnd
