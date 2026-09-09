try {
    $base = 'http://localhost:5173'
    $bearer = 'test-bearer-token'

    $unauth = Invoke-WebRequest -Uri "$base/api/firewall" -UseBasicParsing -ErrorAction SilentlyContinue
    Write-Host 'UNAUTH_STATUS' $unauth.StatusCode
} catch {
    Write-Host 'UNAUTH_STATUS' $_.Exception.Response.StatusCode.value__
}

try {
    $basic = [Convert]::ToBase64String([Text.Encoding]::ASCII.GetBytes('admin:admin'))
    $headers = @{ Authorization = "Basic $basic" }
    $r = Invoke-RestMethod -Uri "$base/api/firewall" -Headers $headers -Method Get
    Write-Host 'BASIC_OK' $r.rules.Count
} catch {
    Write-Host 'BASIC_ERROR' $_.Exception.Message
}

try {
    $headers = @{ Authorization = "Bearer $bearer" }
    $body = @{ action = 'allow'; port = 80; protocol = 'tcp' } | ConvertTo-Json
    $m = Invoke-RestMethod -Uri "$base/api/firewall" -Headers $headers -Method Post -Body $body -ContentType 'application/json'
    Write-Host 'MUTATION_OK' $m.ok
} catch {
    Write-Host 'MUTATION_ERROR' $_.Exception.Message
}
