@echo off
rem MapleSyrup with the phone linked through a Cloudflare tunnel (no certificate
rem warning, no firewall question, works on mobile data too).
cd /d "%~dp0"
MapleSyrup.exe --tunnel %*
if errorlevel 1 pause
