@echo off
rem MapleSyrup, plus a window showing what the vision engine sees.
cd /d "%~dp0"
MapleSyrup.exe --preview %*
if errorlevel 1 pause
