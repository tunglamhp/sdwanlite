try {
    $r = Invoke-WebRequest -Uri 'http://127.0.0.1:18080/api/firewall' -UseBasicParsing
    Write-Host 'STATUS:' $r.StatusCode
    Write-Host 'BODY:' $r.Content
} catch {
    Write-Host 'ERROR:' $_.Exception.Message
}
