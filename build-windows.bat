@echo off
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul 2>&1
cd /d C:\codebase\teletype
set CARGO_BUILD_JOBS=4
set /a tries=0
:retry
set /a tries+=1
echo === build attempt %tries% ===
cargo build -p teletype-desktop
if %ERRORLEVEL%==0 exit /b 0
if %tries% LSS 5 (
    echo === attempt %tries% failed, retrying in 10s ===
    timeout /t 10 /nobreak >nul
    goto retry
)
exit /b %ERRORLEVEL%
