try {
    $base = 'http://localhost:5173'
    $basic = [Convert]::ToBase64String([Text.Encoding]::ASCII.GetBytes('admin:admin'))
    $headers = @{ Authorization = "Basic $basic" }

    $r = Invoke-RestMethod -Uri "$base/api/v1/devices" -Headers $headers -Method Get
    Write-Host 'DEVICES_OK' ($r | Get-Member -MemberType NoteProperty | Measure-Object | Select-Object -ExpandProperty Count)

    if ($r.Count -gt 0) {
        $id = $r[0].id
        $patch = @{ meta = @{ hostname = 'cli-test-host' } }
        $u = Invoke-RestMethod -Uri "$base/api/v1/devices/$id" -Headers $headers -Method Put -Body ($patch | ConvertTo-Json) -ContentType 'application/json'
        Write-Host 'UPDATE_OK' $u.ok
    } else {
        Write-Host 'NO_DEVICES_SKIP_UPDATE'
    }
} catch {
    Write-Host 'DEVICES_ERROR:' $_.Exception.Message
}
