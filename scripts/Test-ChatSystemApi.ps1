[CmdletBinding()]
param([Parameter(Mandatory)][string]$ServerPath)
$ErrorActionPreference='Stop'
$serverPath=(Resolve-Path -LiteralPath $ServerPath).Path
$testDir=Join-Path ([IO.Path]::GetTempPath()) ('apex-chat-api-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDir | Out-Null
$old=@{}; foreach($key in @('VELOX_ADDR','VELOX_SESSION_TOKEN','VELOX_DATA_DIR')) { $old[$key]=[Environment]::GetEnvironmentVariable($key) }
$probe=[Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,0); $probe.Start(); $port=$probe.LocalEndpoint.Port; $probe.Stop()
$env:VELOX_ADDR="127.0.0.1:$port"; $env:VELOX_SESSION_TOKEN=[guid]::NewGuid().ToString('N'); $env:VELOX_DATA_DIR=$testDir
$base="http://127.0.0.1:$port"
function Request($Path,$Method='GET',$Body=$null,$Session=$script:webSession){
    $args=@{Uri="$base$Path";Method=$Method;SkipHttpErrorCheck=$true;TimeoutSec=25}
    if($Session){$args.WebSession=$Session}
    if($null -ne $Body){$args.ContentType='application/json';$args.Body=($Body|ConvertTo-Json -Depth 10 -Compress)}
    Invoke-WebRequest @args
}
function Assert-Status($Response,$Expected){ if([int]$Response.StatusCode -ne $Expected){throw "Expected $Expected, got $($Response.StatusCode): $($Response.Content)"} }
$server=$null
try{
    $server=Start-Process -FilePath $serverPath -WorkingDirectory $testDir -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $testDir 'out.log') -RedirectStandardError (Join-Path $testDir 'err.log')
    for($i=0;$i -lt 50;$i++){try{Invoke-WebRequest "$base/" -TimeoutSec 1 | Out-Null;break}catch{Start-Sleep -Milliseconds 200}}
    foreach($path in @('/system/status','/chat','/chat/options','/chat/no-such-id')){ Assert-Status (Request $path 'GET' $null $null) 401 }
    Assert-Status (Request '/chat' 'POST' @{title='no auth'} $null) 401
    $script:webSession=[Microsoft.PowerShell.Commands.WebRequestSession]::new()
    $script:webSession.Cookies.Add([Net.Cookie]::new('apex_session',$env:VELOX_SESSION_TOKEN,'/','127.0.0.1'))
    $options=(Request '/chat/options').Content|ConvertFrom-Json
    foreach($option in $options.providers){
        if((-not $option.has_key -or -not $option.policy.cloud_allowed) -and -not $option.guidance.Contains('다음 행동')) {throw 'Chat readiness guidance missing.'}
    }
    $status=Request '/system/status';Assert-Status $status 200
    $view=$status.Content|ConvertFrom-Json
    if(-not $view.collected_at){throw 'Missing collection timestamp.'}
    foreach($section in @('services','startup','disks','network','elevated')){
        if($view.$section.state -notin @('ok','unavailable')){throw "Invalid readout: $section"}
        if($view.$section.state -eq 'unavailable' -and -not $view.$section.value){throw 'Missing unavailable reason.'}
    }
    $created=Request '/chat' 'POST' @{title='<img src=x onerror=alert(1)> API fixture'};Assert-Status $created 200
    $conv=$created.Content|ConvertFrom-Json; if(-not $conv.id -or $conv.messages.Count -ne 0){throw 'Conversation JSON contract changed.'}
    $id=$conv.id
    Assert-Status (Request "/chat/$id") 200
    $list=(Request '/chat').Content|ConvertFrom-Json
    if($id -notin $list.conversations.id){throw 'Created conversation absent from list.'}
    Assert-Status (Request '/chat/UPPER') 400
    Assert-Status (Request '/chat/missing-id') 404
    $models=(Request '/models').Content|ConvertFrom-Json
    $model=($models.providers|Where-Object provider -eq 'gpt').model
    $body=@{provider='gpt';model=$model;text='test, must not reach a provider'}
    $denied=Request "/chat/$id/messages" 'POST' $body;Assert-Status $denied 403
    if(-not ($denied.Content|ConvertFrom-Json).error.Contains('다음 행동')){throw 'Guidance missing.'}
    $body.model='stale-model';Assert-Status (Request "/chat/$id/messages" 'POST' $body) 409
    $body.model=$model;$body.text='';Assert-Status (Request "/chat/$id/messages" 'POST' $body) 400
    # Grant only in this disposable data directory, and only when there is no real key.
    $keys=(Request '/keys/status').Content|ConvertFrom-Json
    $keyless=@($models.providers|Where-Object {-not $keys.($_.provider)})
    if($keyless.Count -gt 0){
        $provider=$keyless[0].provider
        Assert-Status (Request '/policies/consent' 'POST' @{provider=$provider;scope='minimal'}) 200
        $missing=Request "/chat/$id/messages" 'POST' @{provider=$provider;model=$keyless[0].model;text='keyless test'}
        Assert-Status $missing 412
        if(-not ($missing.Content|ConvertFrom-Json).error.Contains('다음 행동')){throw 'Missing-key guidance absent.'}
    }else{Write-Host 'SKIP missing-key case: real credentials exist for every provider; no cloud call made.'}
    $loaded=(Request "/chat/$id").Content|ConvertFrom-Json
    if($loaded.messages.Count -ne 0){throw 'Denied requests wrote conversation messages.'}
    Assert-Status (Request "/chat/$id" 'DELETE') 200
    Assert-Status (Request "/chat/$id") 404
    Write-Host 'PASS: API authentication, SystemView schema/reasons, chat create/list/load/delete, consent denial, stale model, empty input, keyless guidance. No cloud calls.'
}finally{
    if($server -and -not $server.HasExited){Stop-Process -Id $server.Id}
    foreach($key in $old.Keys){[Environment]::SetEnvironmentVariable($key,$old[$key])}
}
