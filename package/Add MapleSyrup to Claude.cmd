@echo off
rem Adds MapleSyrup to the Claude desktop app: Claude then sees the game through it,
rem in any chat. Quit the Claude app completely (its icon by the clock -> Quit) and
rem open it again afterwards.
"%~dp0MapleSyrup.exe" --install-claude
pause
