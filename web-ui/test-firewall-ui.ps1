try {
    $base = 'http://localhost:5173'
    $basic = [Convert]::ToBase64String([Text.Encoding]::ASCII.GetBytes('admin:admin'))
    $headers = @{ Authorization = "Basic $basic" }

    $r = Invoke-RestMethod -Uri "$base/api/firewall" -Headers $headers -Method Get
    $rules = $r.rules
    Write-Host 'LIST_OK' $rules.Count

    $new = @{ action = 'allow'; source = '10.0.0.0/8'; destination = '0.0.0.0/0'; protocol = 'tcp'; port = '443'; comment = 'test allow' }
    $c = Invoke-RestMethod -Uri "$base/api/firewall" -Headers $headers -Method Post -Body ($new | ConvertTo-Json) -ContentType 'application/json'
    Write-Host 'CREATE_OK' $c.ok

    $idx = ($c.rules | Where-Object comment -eq 'test allow' | Select-Object -First 1).GetType().GetProperty('index')?.GetValue($c.rules | Where-Object comment -eq 'test allow' | Select-Object -First 1, $null)
    if (-not $idx) { $idx = ($c.rules | Measure-Object | Select-Object -ExpandProperty Count) - 1 }
    $updated = $c.rules[$idx]
    $updated.comment = 'test allow updated'
    $u = Invoke-RestMethod -Uri "$base/api/firewall/$idx" -Headers $headers -Method Put -Body ($updated | ConvertTo-Json) -ContentType 'application/json'
    Write-Host 'UPDATE_OK' $u.ok

    $d = Invoke-RestMethod -Uri "$base/api/firewall/$idx" -Headers $headers -Method Delete
    Write-Host 'DELETE_OK' $d.ok
} catch {
    Write-Host 'FIREWALL_UI_ERROR:' $_.Exception.Message
}
