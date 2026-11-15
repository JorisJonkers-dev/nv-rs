@echo off
rem Pick the folder of your Fallout: New Vegas install (the one with Data\FalloutNV.esm, or its Data folder) and start
rem the game on it. nv-rs remembers the folder, so Play.exe finds it next time. The folder is only read.
start "" powershell -NoProfile -ExecutionPolicy Bypass -STA -WindowStyle Hidden -File "%~dp0Choose game folder.ps1"
