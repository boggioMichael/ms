@echo off
rem MapleSyrup for recording or streaming: its dog and panel show up in OBS
rem and screen recordings (normally they keep out of captures), and what the
rem phone's microphone hears is kept in the session folder (mic.wav).
rem A new version waiting next to it is put in place first.
cd /d "%~dp0"
taskkill /IM MapleSyrup.exe /F >nul 2>nul
taskkill /IM "Maplesyrup (2).exe" /F >nul 2>nul
if exist "MapleSyrup-new.exe" timeout /t 2 /nobreak >nul
if exist "MapleSyrup-new.exe" move /Y "MapleSyrup-new.exe" "MapleSyrup.exe" >nul
if exist "Maplesyrup (2).exe" del "Maplesyrup (2).exe" >nul 2>nul
MapleSyrup.exe --overlay-on-stream --record-mic %*
if errorlevel 1 pause
