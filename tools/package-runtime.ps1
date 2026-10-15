param(
  [Parameter(Mandatory=$true)][string]$Destination,
  [string]$NodePath = 'C:\Program Files\nodejs\node.exe'
)
$ErrorActionPreference='Stop'
$destinationPath=[IO.Path]::GetFullPath($Destination)
if(-not(Test-Path -LiteralPath $destinationPath -PathType Container)){throw '先创建明确的交付目录'}
$node=(Resolve-Path -LiteralPath $NodePath).ProviderPath
$signature=Get-AuthenticodeSignature -LiteralPath $node
if($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'OpenJS Foundation'){throw '请使用已核验的官方 Node.js Windows 运行时'}
$version=& $node --version
if($version -ne 'v24.15.0'){throw '本交付脚本对应 Node.js v24.15.0；更换版本时需同步许可证并重新验证'}
$runtime=Join-Path $destinationPath 'runtime'
New-Item -ItemType Directory -Path $runtime -Force | Out-Null
$target=Join-Path $runtime 'node.exe'
if((Test-Path -LiteralPath $target) -and (Get-FileHash -LiteralPath $target).Hash -ne (Get-FileHash -LiteralPath $node).Hash){throw '目标运行时与已验证版本不同，原文件保留'}
Copy-Item -LiteralPath $node -Destination $target
$notices=Join-Path (Split-Path $PSScriptRoot -Parent) 'THIRD_PARTY_NOTICES.md'
Copy-Item -LiteralPath $notices -Destination (Join-Path $destinationPath 'THIRD_PARTY_NOTICES.md')
if((Get-FileHash -LiteralPath $target).Hash -ne (Get-FileHash -LiteralPath $node).Hash){throw '运行时复制核验失败'}
Write-Output "RUNTIME_PACKAGE_PASS version=$version signer=OpenJS hash=MATCH"
