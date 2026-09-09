@echo off
cd /d A:\web\sdwan\sdwanlite\web-ui
(npm run lint > A:\web\sdwan\sdwanlite\webui-check.log 2>&1) & (npx vitest run --reporter=basic >> A:\web\sdwan\sdwanlite\webui-check.log 2>&1)
echo EXITCODE=%ERRORLEVEL% >> A:\web\sdwan\sdwanlite\webui-check.log
