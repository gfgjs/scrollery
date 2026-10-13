@echo off
echo =======================================================
echo Chinese-CLIP ViT-Large to ONNX Conversion Script
echo =======================================================
echo.

:: Check if Python is installed
py --version >nul 2>&1
if %ERRORLEVEL% NEQ 0 (
    echo [ERROR] Python is not installed or not in PATH!
    echo Please install Python 3.10+ from the Microsoft Store or python.org, then try again.
    pause
    exit /b 1
)

:: Set paths
set SCRIPT_DIR=%~dp0
:: Find models dir based on standard location or APPDATA
set MODELS_DIR=C:\Users\gf\AppData\Roaming\com.scrollery.app\models
set PT_FILE=%MODELS_DIR%\clip_cn_vit-l-14.pt

if not exist "%PT_FILE%" (
    echo [ERROR] Could not find clip_cn_vit-l-14.pt in %MODELS_DIR%
    echo Please make sure you placed the downloaded .pt file in the models folder.
    pause
    exit /b 1
)

echo [1/3] Setting up Python virtual environment...
if not exist "venv" (
    py -m venv venv
)
call venv\Scripts\activate

echo [2/3] Installing PyTorch and ONNX dependencies...
python -m pip install --upgrade pip
pip install torch torchvision torchaudio onnx onnxscript --upgrade --index-url https://download.pytorch.org/whl/cpu

echo [3/3] Cloning OFA-Sys/Chinese-CLIP repository for conversion scripts...
if not exist "Chinese-CLIP" (
    git clone https://github.com/OFA-Sys/Chinese-CLIP.git
)

:: 上游 Chinese-CLIP 的 pytorch_to_onnx.py 会调用 onnxmltools 转换 FP16；
:: 该依赖与此脚本安装的 PyTorch/ONNX 版本不兼容。导出前需手动跳过 FP16 转换：
::   1. 删除或注释顶部的 from onnxmltools.utils import convert_float_to_float16；
::   2. 删除或注释 text_fp16_onnx_path 对应的文本转换块；
::   3. 删除或注释 vision_fp16_onnx_path 对应的图像转换块。
:: 文件：Chinese-CLIP\cn_clip\deploy\pytorch_to_onnx.py。保留 FP32 导出。

echo.
echo Starting ONNX export...
set PYTHONPATH=%PYTHONPATH%;%SCRIPT_DIR%Chinese-CLIP;%SCRIPT_DIR%Chinese-CLIP\cn_clip
set PYTHONIOENCODING=utf-8

python Chinese-CLIP\cn_clip\deploy\pytorch_to_onnx.py ^
    --model-arch ViT-L-14 ^
    --pytorch-ckpt-path "%PT_FILE%" ^
    --save-onnx-path "%MODELS_DIR%\cn-clip-vit-l14" ^
    --convert-text ^
    --convert-vision

if %ERRORLEVEL% EQU 0 (
    echo.
    echo =======================================================
    echo [SUCCESS] ONNX models generated successfully!
    echo Look for cn-clip-vit-l14.img.fp32.onnx and cn-clip-vit-l14.txt.fp32.onnx in %MODELS_DIR%
    echo =======================================================
) else (
    echo.
    echo [FAILED] Conversion failed. Please check the error messages above.
)
pause
