try {
    $base = 'http://localhost:5173'
    $basic = [Convert]::ToBase64String([Text.Encoding]::ASCII.GetBytes('admin:admin'))
    $headers = @{ Authorization = "Basic $basic" }

    $r = Invoke-RestMethod -Uri "$base/api/lb" -Headers $headers -Method Get
    Write-Host 'LB_OK' ($r.tcp.Count + $r.http.Count)

    foreach ($pool in $r.tcp) {
        Write-Host 'TCP_POOL' $pool.name $pool.algorithm $pool.backends.Count
        foreach ($be in $pool.backends) {
            Write-Host ('BACKEND ' + $be.addr + ' healthy=' + $be.healthy)
        }
    }

    foreach ($pool in $r.http) {
        Write-Host 'HTTP_POOL' $pool.name $pool.routes.Count
        foreach ($route in $pool.routes) {
            Write-Host ('ROUTE ' + $route.host + $route.path_prefix + ' algo=' + $route.algorithm + ' backends=' + $route.backends)
        }
    }
} catch {
    Write-Host 'FAILOVER_ERROR:' $_.Exception.Message
}
