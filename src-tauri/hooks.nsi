!macro NSIS_HOOK_POSTINSTALL
  # Automatically register SpotGlow install directory to User PATH
  ExecWait 'powershell.exe -NoProfile -WindowStyle Hidden -Command "$d=\"$INSTDIR\"; $p=[Environment]::GetEnvironmentVariable(\"Path\",\"User\"); $items=if($p){$p -split \";\"}else{@()}; $found=$false; foreach($i in $items){if($i.TrimEnd(\"\\\") -ieq $d.TrimEnd(\"\\\")){$found=$true;break}}; if(-not $found){$n=if($p){\"$p;$d\"}else{$d}; [Environment]::SetEnvironmentVariable(\"Path\",$n,\"User\")}"'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  # Automatically remove SpotGlow install directory from User PATH on uninstall
  ExecWait 'powershell.exe -NoProfile -WindowStyle Hidden -Command "$d=\"$INSTDIR\"; $p=[Environment]::GetEnvironmentVariable(\"Path\",\"User\"); if($p){$items=$p -split \";\" | Where-Object {$_ -and ($_.TrimEnd(\"\\\") -ine $d.TrimEnd(\"\\\"))}; $n=$items -join \";\"; [Environment]::SetEnvironmentVariable(\"Path\",$n,\"User\")}"'
!macroend
