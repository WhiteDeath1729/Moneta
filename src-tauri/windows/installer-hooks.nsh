!macro NSIS_HOOK_POSTINSTALL
  SetOutPath "$INSTDIR"
  CreateDirectory "$SMPROGRAMS\Moneta"
  CreateShortCut "$SMPROGRAMS\Moneta\Bookmark File.lnk" "$INSTDIR\moneta.exe" "--moneta-action=bookmark-file" "$INSTDIR\moneta.exe" 0 SW_SHOWNORMAL "CONTROL|SHIFT|O"
  CreateShortCut "$SMPROGRAMS\Moneta\Capture Screenshot.lnk" "$INSTDIR\moneta.exe" "--moneta-action=screenshot" "$INSTDIR\moneta.exe" 0 SW_SHOWNORMAL "CONTROL|SHIFT|S"
  CreateShortCut "$SMPROGRAMS\Moneta\Bookmark Selection.lnk" "$INSTDIR\moneta.exe" "--moneta-action=selection" "$INSTDIR\moneta.exe" 0 SW_SHOWNORMAL "CONTROL|SHIFT|B"
  CreateShortCut "$SMPROGRAMS\Moneta\Search Bookmarks.lnk" "$INSTDIR\moneta.exe" "--moneta-action=search" "$INSTDIR\moneta.exe" 0 SW_SHOWNORMAL "CONTROL|SHIFT|F"
  CreateShortCut "$SMPROGRAMS\Moneta\Analyze Screen.lnk" "$INSTDIR\moneta.exe" "--moneta-action=screen-search" "$INSTDIR\moneta.exe" 0 SW_SHOWNORMAL "CONTROL|SHIFT|R"
  CreateShortCut "$SMPROGRAMS\Moneta\Bookmark Graph.lnk" "$INSTDIR\moneta.exe" "--moneta-action=graph" "$INSTDIR\moneta.exe" 0 SW_SHOWNORMAL "CONTROL|SHIFT|G"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  Delete "$SMPROGRAMS\Moneta\Bookmark File.lnk"
  Delete "$SMPROGRAMS\Moneta\Capture Screenshot.lnk"
  Delete "$SMPROGRAMS\Moneta\Bookmark Selection.lnk"
  Delete "$SMPROGRAMS\Moneta\Search Bookmarks.lnk"
  Delete "$SMPROGRAMS\Moneta\Analyze Screen.lnk"
  Delete "$SMPROGRAMS\Moneta\Bookmark Graph.lnk"
  RMDir "$SMPROGRAMS\Moneta"
!macroend
