@echo off
rem MapleSyrup with Claude: writes the Claude folder beside MapleSyrup.exe, then starts
rem Claude there (Claude Code, installed from claude.ai the first time), with MapleSyrup's
rem live view of the game and its voice on the phone.
cd /d "%~dp0"
echo.| "%~dp0MapleSyrup.exe" --setup-claude >nul
if not exist "%~dp0Claude\Start Claude.cmd" (
  echo MapleSyrup couldn't write the Claude folder beside it: is this folder writable?
  pause
  exit /b 1
)
call "%~dp0Claude\Start Claude.cmd"
