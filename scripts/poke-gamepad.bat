@echo off
REM Smoke test for the gamepad path in w3stream-helper.
REM Spawns the installed helper, talks to it over the Chrome Native
REM Messaging frame protocol from PowerShell, exercises the suspend +
REM dpad + button sequence, and prints the responses.
REM
REM Streamer usage:
REM   1. Install w3stream-helper (drivers + reboot).
REM   2. Open https://gamepad-tester.com/ in Chrome.
REM   3. Double-click this .bat.
REM   4. While the script's 5-second suspend window is active, hold the
REM      physical stick. gamepad-tester should NOT show stick movement
REM      on the virtual pad - that confirms HidHide + forwarder-suspend
REM      are working together.
REM   5. After suspend ends, physical input should forward again.

setlocal
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0poke-gamepad.ps1" %*
exit /b %errorlevel%
