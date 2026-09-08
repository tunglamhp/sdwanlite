try {
    $r = Invoke-WebRequest -Uri 'http://localhost:5173/api/firewall' -UseBasicParsing
    Write-Host 'STATUS:' $r.StatusCode
    Write-Host 'BODY:' $r.Content
} catch {
    Write-Host 'ERROR:' $_.Exception.Message
}
