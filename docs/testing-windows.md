# Testing Grammable on Windows

All commands are for **PowerShell**. You don't need admin rights.

## 1. Download the installer

**From a browser:** open [Actions → Build](https://github.com/UdayaLakmal/Grammable-Desktop/actions/workflows/build.yml), then open the newest green run on `dev`. Download **grammable-windows** from the *Artifacts* section at the bottom, and unzip it.

**With the GitHub CLI** (if `gh` is installed and signed in):

```powershell
gh run download -R UdayaLakmal/Grammable-Desktop -n grammable-windows -D "$env:TEMP\grammable"
```

## 2. Install

```powershell
& (Get-ChildItem "$env:TEMP\grammable\*setup.exe" | Select-Object -First 1).FullName
```

- **SmartScreen warning:** you'll see "Windows protected your PC", because the installer isn't signed yet. Click **More info → Run anyway**.
- **Install location:** Grammable installs for your user only, in `%LOCALAPPDATA%\Grammable`.

## 3. Run with the debug log

Quit Grammable if it's running, then start it with debug logging:

```powershell
Stop-Process -Name grammable -ErrorAction SilentlyContinue
$exe = (Get-ChildItem "$env:LOCALAPPDATA" -Recurse -Filter grammable.exe -ErrorAction SilentlyContinue | Select-Object -First 1).FullName
$exe
$env:GRAMMABLE_DEBUG = "1"; Start-Process $exe
```

The log goes to `%APPDATA%\Grammable\debug.log`. It's cleared each time Grammable starts.

It records:
- app names, character counts and outcomes;
- never the text you select.

## 4. Things to try

1. **Shortcut:** select a sentence in Notepad and press **Ctrl+Alt+G**. The menu should open next to the mouse.
2. **G button:** select text by dragging the mouse, or by double-clicking a word. A round **G** should appear next to the mouse; click it.
3. **Run a preset:** choose **Fix Grammar** or **Improve Writing**, review the result, then click **Replace**. The text in Notepad should be replaced.
4. **Other apps:** try the same in Edge or Chrome, Word, and VS Code.
5. **Skipped places:** nothing should happen in a password field, or in Windows Terminal / PowerShell.
6. **Tray icon:** right-click it to open Settings, and check **AI Backends**.

## 5. Send the results back

**Copy the debug log to the clipboard**, then paste it into the chat:

```powershell
Get-Content "$env:APPDATA\Grammable\debug.log" | Set-Clipboard
```

**Memory use** of Grammable and its WebView2 processes:

```powershell
$root = (Get-Process grammable).Id
$all = Get-CimInstance Win32_Process
$ids = @($root); $added = $true
while ($added) {
  $added = $false
  foreach ($p in $all) { if ($ids -contains $p.ParentProcessId -and $ids -notcontains $p.ProcessId) { $ids += $p.ProcessId; $added = $true } }
}
$procs = Get-Process -Id $ids
$procs | Select-Object Name, Id, @{n='Private MB';e={[math]::Round($_.PrivateMemorySize64/1MB)}} | Format-Table
"Total: {0} MB" -f [math]::Round(($procs | Measure-Object PrivateMemorySize64 -Sum).Sum/1MB)
```

**Which AIs Grammable can find:**

```powershell
& $exe --status
```
