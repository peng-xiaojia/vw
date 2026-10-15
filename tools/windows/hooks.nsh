!macro NSIS_HOOK_PREINSTALL
  ; A running vw-app.exe locks the install file and aborts the copy; close it first.
  nsExec::Exec 'taskkill /F /T /IM vw-app.exe'
  Pop $R9
  Sleep 800
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::Exec 'taskkill /F /T /IM vw-app.exe'
  Pop $R9
  Sleep 800
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Advertise Open With. Never write Explorer's protected UserChoice key.
  ReadRegDWORD $R0 HKCU "Software\vw" "FileAssociationsConfigured"
  StrCmp $R0 1 association_selected
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe" "FriendlyAppName" "熊猫查看器"
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\shell\open\command" "" '$\"$INSTDIR\vw-app.exe$\" $\"%1$\"'
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".txt" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".log" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".md" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".markdown" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".html" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".htm" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".json" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".jsonl" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".csv" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".tsv" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".zip" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".png" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".jpg" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".jpeg" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".gif" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".webp" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".bmp" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".ini" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".toml" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".cfg" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".pdf" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".docx" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".xlsx" ""
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" ".pptx" ""
  Goto association_done
association_selected:
  ; A previous in-app choice wins over the installer's default list.
  EnumRegValue $R1 HKCU "Software\vw\AssociationSelection" 0
  StrCmp $R1 "" association_done
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe" "FriendlyAppName" "熊猫查看器"
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\shell\open\command" "" '$\"$INSTDIR\vw-app.exe$\" $\"%1$\"'
  DeleteRegKey HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes"
  StrCpy $R2 0
association_copy:
  EnumRegValue $R1 HKCU "Software\vw\AssociationSelection" $R2
  StrCmp $R1 "" association_done
  WriteRegStr HKCU "Software\Classes\Applications\vw-app.exe\SupportedTypes" "$R1" ""
  IntOp $R2 $R2 + 1
  Goto association_copy
association_done:
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Leave another installation's registration intact.
  ReadRegStr $R0 HKCU "Software\Classes\Applications\vw-app.exe\shell\open\command" ""
  StrCmp $R0 '$\"$INSTDIR\vw-app.exe$\" $\"%1$\"' 0 +2
  DeleteRegKey HKCU "Software\Classes\Applications\vw-app.exe"
!macroend
