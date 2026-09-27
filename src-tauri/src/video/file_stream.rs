//! 将宿主移交的只读文件句柄包装为 MF 字节流；MF 不再按路径重开源视频。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, Mutex};

use windows::core::{implement, Error, Interface, HRESULT, HSTRING};
use windows::Win32::Foundation::{
    E_NOTIMPL, E_POINTER, STG_E_ACCESSDENIED, STG_E_INVALIDFUNCTION, S_FALSE, S_OK,
};
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFByteStream, MFCreateMFByteStreamOnStream, MF_BYTESTREAM_CONTENT_TYPE,
};
use windows::Win32::System::Com::{
    ISequentialStream_Impl, IStream, IStream_Impl, LOCKTYPE, STATFLAG, STATSTG, STGC, STGM_READ,
    STGTY_STREAM, STREAM_SEEK, STREAM_SEEK_CUR, STREAM_SEEK_END, STREAM_SEEK_SET,
};

use crate::error::{AppError, Result};

#[implement(IStream)]
struct ReadOnlyFileStream {
    file: Arc<Mutex<File>>,
    position: Mutex<u64>,
    size: u64,
}

impl ISequentialStream_Impl for ReadOnlyFileStream_Impl {
    fn Read(&self, pv: *mut std::ffi::c_void, cb: u32, pcbread: *mut u32) -> HRESULT {
        if cb > 0 && pv.is_null() {
            return E_POINTER;
        }
        if !pcbread.is_null() {
            // SAFETY: COM 调用方提供可写的 u32 指针；非空时写入实际读取量。
            unsafe { *pcbread = 0 };
        }
        if cb == 0 {
            return S_OK;
        }
        let mut position = self.position.lock().unwrap_or_else(|e| e.into_inner());
        let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
        if file.seek(SeekFrom::Start(*position)).is_err() {
            return STG_E_INVALIDFUNCTION;
        }
        // SAFETY: pv 非空，COM 的 Read 契约要求从该地址起至少 cb 字节可写。
        let buffer = unsafe { std::slice::from_raw_parts_mut(pv.cast::<u8>(), cb as usize) };
        match file.read(buffer) {
            Ok(read) => {
                *position += read as u64;
                if !pcbread.is_null() {
                    // SAFETY: 上方已检查非空，read<=cb<=u32::MAX。
                    unsafe { *pcbread = read as u32 };
                }
                if read == cb as usize {
                    S_OK
                } else {
                    S_FALSE
                }
            }
            Err(_) => STG_E_INVALIDFUNCTION,
        }
    }

    fn Write(&self, _pv: *const std::ffi::c_void, _cb: u32, pcbwritten: *mut u32) -> HRESULT {
        if !pcbwritten.is_null() {
            // SAFETY: COM 调用方提供可写的 u32 指针。
            unsafe { *pcbwritten = 0 };
        }
        STG_E_ACCESSDENIED
    }
}

impl IStream_Impl for ReadOnlyFileStream_Impl {
    fn Seek(
        &self,
        dlibmove: i64,
        dworigin: STREAM_SEEK,
        plibnewposition: *mut u64,
    ) -> windows::core::Result<()> {
        let mut position = self.position.lock().unwrap_or_else(|e| e.into_inner());
        let base = match dworigin {
            STREAM_SEEK_SET => 0,
            STREAM_SEEK_CUR => *position,
            STREAM_SEEK_END => self.size,
            _ => return Err(Error::from(STG_E_INVALIDFUNCTION)),
        };
        let next = (base as i128) + (dlibmove as i128);
        if !(0..=u64::MAX as i128).contains(&next) {
            return Err(Error::from(STG_E_INVALIDFUNCTION));
        }
        *position = next as u64;
        if !plibnewposition.is_null() {
            // SAFETY: COM 调用方提供可写的 u64 指针。
            unsafe { *plibnewposition = *position };
        }
        Ok(())
    }

    fn SetSize(&self, _libnewsize: u64) -> windows::core::Result<()> {
        Err(Error::from(STG_E_ACCESSDENIED))
    }

    fn CopyTo(
        &self,
        _pstm: Option<&IStream>,
        _cb: u64,
        _pcbread: *mut u64,
        _pcbwritten: *mut u64,
    ) -> windows::core::Result<()> {
        Err(Error::from(E_NOTIMPL))
    }

    fn Commit(&self, _grfcommitflags: &STGC) -> windows::core::Result<()> {
        Ok(())
    }

    fn Revert(&self) -> windows::core::Result<()> {
        Err(Error::from(E_NOTIMPL))
    }

    fn LockRegion(
        &self,
        _liboffset: u64,
        _cb: u64,
        _dwlocktype: &LOCKTYPE,
    ) -> windows::core::Result<()> {
        Err(Error::from(E_NOTIMPL))
    }

    fn UnlockRegion(
        &self,
        _liboffset: u64,
        _cb: u64,
        _dwlocktype: u32,
    ) -> windows::core::Result<()> {
        Err(Error::from(E_NOTIMPL))
    }

    fn Stat(&self, pstatstg: *mut STATSTG, _grfstatflag: &STATFLAG) -> windows::core::Result<()> {
        if pstatstg.is_null() {
            return Err(Error::from(E_POINTER));
        }
        let stat = STATSTG {
            r#type: STGTY_STREAM.0 as u32,
            cbSize: self.size,
            grfMode: STGM_READ,
            ..STATSTG::default()
        };
        // SAFETY: 指针已检查非空，COM 调用方提供一个可写 STATSTG。
        unsafe { *pstatstg = stat };
        Ok(())
    }

    fn Clone(&self) -> windows::core::Result<IStream> {
        let position = *self.position.lock().unwrap_or_else(|e| e.into_inner());
        Ok(ReadOnlyFileStream {
            file: Arc::clone(&self.file),
            position: Mutex::new(position),
            size: self.size,
        }
        .into())
    }
}

pub(super) fn byte_stream(file: Arc<Mutex<File>>, format: &str) -> Result<IMFByteStream> {
    let size = file
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .metadata()?
        .len();
    let stream: IStream = ReadOnlyFileStream {
        file,
        position: Mutex::new(0),
        size,
    }
    .into();
    // SAFETY: IStream 是由本函数创建的 COM 对象，MF 取得其引用计数所有权。
    let bytes = unsafe { MFCreateMFByteStreamOnStream(&stream) }
        .map_err(|e| AppError::os("MF 字节流创建失败 | byte stream creation failed", e))?;
    let mime = match format.to_ascii_lowercase().as_str() {
        "mp4" | "m4v" | "mov" | "3gp" | "3g2" => "video/mp4",
        "wmv" | "asf" => "video/x-ms-wmv",
        "avi" => "video/x-msvideo",
        "ts" | "mts" | "m2ts" => "video/mp2t",
        "mpg" | "mpeg" => "video/mpeg",
        _ => return Err(AppError::UnsupportedFormat(format.to_owned())),
    };
    let attrs: IMFAttributes = bytes
        .cast()
        .map_err(|e| AppError::os("MF 字节流属性不可用 | byte stream attributes missing", e))?;
    // SAFETY: 属性和值都由本进程提供，格式仅来自已知原生容器白名单。
    unsafe { attrs.SetString(&MF_BYTESTREAM_CONTENT_TYPE, &HSTRING::from(mime)) }
        .map_err(|e| AppError::os("MF 媒体类型设置失败 | content type setup failed", e))?;
    Ok(bytes)
}
