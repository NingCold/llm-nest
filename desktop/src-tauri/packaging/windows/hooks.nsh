; Rename only legacy shortcuts that still point to this installation.
; Runs after successful file/registry installation, including /UPDATE installs.
!macro LLMN_RENAME_SHORTCUT DIRECTORY
  !insertmacro IsShortcutTarget "${DIRECTORY}\LLM-Nest.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  Pop $0
  ${If} $0 = 1
    ${If} ${FileExists} "${DIRECTORY}\${PRODUCTNAME}.lnk"
      !insertmacro IsShortcutTarget "${DIRECTORY}\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
      Pop $0
      ${If} $0 = 1
        Delete "${DIRECTORY}\LLM-Nest.lnk"
      ${EndIf}
    ${Else}
      Rename "${DIRECTORY}\LLM-Nest.lnk" "${DIRECTORY}\${PRODUCTNAME}.lnk"
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro LLMN_RENAME_SHORTCUT "$SMPROGRAMS"
  !insertmacro LLMN_RENAME_SHORTCUT "$DESKTOP"
!macroend
