@echo off
REM Smoke test for the legacy keystroke path in w3stream-helper.
REM Spawns the installed helper, talks to it over the Chrome Native
REM Messaging frame protocol from PowerShell, exercises the
REM hello/health/enabled/trigger/panic round-trip, and prints responses.
REM
REM Streamer usage:
REM   1. Install w3stream-helper.
REM   2. Open a text window (Notepad, etc.) and focus it — the script
REM      will fire test_type_hi which sends keystrokes via SendInput to
REM      whatever window has focus.
REM   3. Double-click this .bat or run from a shell.
REM   4. After the 3-second countdown, the script fires test_type_hi.
REM      You should see "HI" typed into the focused window.
REM
REM This script verifies the keyboard-only path works independent of
REM ViGEmBus/HidHide, so a fresh streamer who hasn't rebooted after
REM driver install can still confirm the basics.

setlocal
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0poke-helper.ps1" %*
exit /b %errorlevel%
