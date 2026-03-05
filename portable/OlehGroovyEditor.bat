@echo off
setlocal
set SCRIPT_DIR=%~dp0
set EXE=%SCRIPT_DIR%oleh-groovy-editor.exe

if not exist "%EXE%" (
    echo [Oleh Groovy Editor] Missing executable:
    echo %EXE%
    exit /b 1
)

start "" "%EXE%" %*
endlocal
