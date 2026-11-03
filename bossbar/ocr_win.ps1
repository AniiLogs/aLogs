# Runs Windows' built-in OCR (Windows.Media.Ocr) on image files.
#   ocr_win.ps1 a.png b.png      -> one line per image: "<path><TAB><text>"
#   ocr_win.ps1 -Server          -> stays running: first "READY<TAB><language>" (en-US; nothing without en-US OCR),
#                                   then reads one image path per line on stdin, answers the same way
# Recognized lines are joined with ' / '.
param([switch]$Server, [Parameter(ValueFromRemainingArguments = $true)][string[]]$Paths)

[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
[Console]::InputEncoding = [System.Text.Encoding]::UTF8
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$asTaskGeneric = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
    $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and
    $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
function Await($op, [Type]$type) {
    $task = $asTaskGeneric.MakeGenericMethod($type).Invoke($null, @($op))
    $task.Wait(-1) | Out-Null
    $task.Result
}
[Windows.Storage.StorageFile, Windows.Storage, ContentType = WindowsRuntime] | Out-Null
[Windows.Media.Ocr.OcrEngine, Windows.Foundation, ContentType = WindowsRuntime] | Out-Null
[Windows.Graphics.Imaging.BitmapDecoder, Windows.Graphics, ContentType = WindowsRuntime] | Out-Null
[Windows.Globalization.Language, Windows.Globalization, ContentType = WindowsRuntime] | Out-Null

$engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromLanguage([Windows.Globalization.Language]::new('en-US'))

function Read-Text([string]$p) {
    try {
        $full = (Resolve-Path $p).Path
        $file = Await ([Windows.Storage.StorageFile]::GetFileFromPathAsync($full)) ([Windows.Storage.StorageFile])
        $stream = Await ($file.OpenAsync([Windows.Storage.FileAccessMode]::Read)) ([Windows.Storage.Streams.IRandomAccessStream])
        $decoder = Await ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder])
        $bitmap = Await ($decoder.GetSoftwareBitmapAsync()) ([Windows.Graphics.Imaging.SoftwareBitmap])
        $result = Await ($engine.RecognizeAsync($bitmap)) ([Windows.Media.Ocr.OcrResult])
        $stream.Dispose()
        return ($result.Lines | ForEach-Object { $_.Text }) -join ' / '
    } catch { return '' }
}

if ($Server) {
    # the language Windows OCR reads in after a tab, as ocr-helper.exe says it (reader.ready_language): en-US, or
    # nothing when this PC has no en-US OCR (then every line comes back empty)
    $language = if ($engine) { $engine.RecognizerLanguage.LanguageTag } else { '' }
    [Console]::Out.WriteLine("READY`t$language"); [Console]::Out.Flush()
    while ($null -ne ($line = [Console]::In.ReadLine())) {
        $line = $line.Trim()
        if ($line -eq '') { continue }
        [Console]::Out.WriteLine("$line`t$(Read-Text $line)"); [Console]::Out.Flush()
    }
} else {
    foreach ($p in $Paths) { Write-Output "$p`t$(Read-Text $p)" }
}
