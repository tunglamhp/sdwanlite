try {
    $r = Invoke-WebRequest -Uri 'http://localhost:5173/' -UseBasicParsing
    Write-Host 'ROOT_STATUS:' $r.StatusCode
    Write-Host 'ROOT_BODY_START:' $r.Content.Substring(0, [Math]::Min(200, $r.Content.Length))
} catch {
    Write-Host 'ROOT_ERROR:' $_.Exception.Message
}
