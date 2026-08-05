#[cfg(not(target_os = "macos"))]
pub(crate) use rawlib::{DecodeOptions, ImageFormat, RawProcessor};

#[cfg(target_os = "macos")]
mod macos {
    use rsraw_sys as ffi;
    use std::ffi::{CStr, CString};
    use std::fmt;
    use std::path::Path;

    #[derive(Debug, Clone)]
    pub(crate) struct RawError {
        code: i32,
        message: String,
    }

    impl fmt::Display for RawError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "LibRaw error {}: {}", self.code, self.message)
        }
    }

    impl std::error::Error for RawError {}

    type Result<T> = std::result::Result<T, RawError>;

    #[derive(Debug, Clone)]
    pub(crate) struct ThumbnailData {
        pub(crate) format: ImageFormat,
        pub(crate) width: u16,
        pub(crate) height: u16,
        pub(crate) colors: u16,
        pub(crate) bits: u16,
        pub(crate) data: Vec<u8>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum ImageFormat {
        Jpeg,
        Bitmap,
        Unknown(i32),
    }

    impl ImageFormat {
        fn from_code(code: i32) -> Self {
            match code as u32 {
                ffi::LibRaw_image_formats_LIBRAW_IMAGE_JPEG => Self::Jpeg,
                ffi::LibRaw_image_formats_LIBRAW_IMAGE_BITMAP => Self::Bitmap,
                _ => Self::Unknown(code),
            }
        }
    }

    #[derive(Debug, Clone, Copy)]
    pub(crate) struct DecodeOptions {
        pub(crate) half_size: bool,
        pub(crate) demosaic_quality: i32,
        pub(crate) output_bps: i32,
        pub(crate) no_auto_bright: bool,
        pub(crate) output_color: i32,
        pub(crate) linear_gamma: bool,
        pub(crate) use_camera_wb: bool,
    }

    pub(crate) struct RawProcessor {
        data: *mut ffi::libraw_data_t,
    }

    impl RawProcessor {
        pub(crate) fn new() -> Result<Self> {
            let data =
                unsafe { ffi::libraw_init(ffi::LibRaw_constructor_flags_LIBRAW_OPTIONS_NONE) };
            if data.is_null() {
                return Err(RawError {
                    code: -1,
                    message: "Failed to initialize LibRaw".to_string(),
                });
            }
            Ok(Self { data })
        }

        pub(crate) fn open_file<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
            let path = path.as_ref();
            if !path.exists() {
                return Err(RawError {
                    code: -1,
                    message: format!("File does not exist: {}", path.display()),
                });
            }
            let path_text = path.to_str().ok_or_else(|| RawError {
                code: -1,
                message: format!("Invalid path encoding: {}", path.display()),
            })?;
            let c_path = CString::new(path_text).map_err(|error| RawError {
                code: -1,
                message: format!("Path contains a null byte: {error}"),
            })?;
            let status = unsafe { ffi::libraw_open_file(self.data, c_path.as_ptr()) };
            self.check(status)
        }

        pub(crate) fn unpack_thumb(&mut self) -> Result<()> {
            let status = unsafe { ffi::libraw_unpack_thumb(self.data) };
            self.check(status)
        }

        pub(crate) fn get_thumbnail(&self) -> Result<ThumbnailData> {
            let mut error_code = 0;
            let image = unsafe { ffi::libraw_dcraw_make_mem_thumb(self.data, &mut error_code) };
            self.copy_image(image, error_code)
        }

        pub(crate) fn extract_image_with_options<P: AsRef<Path>>(
            path: P,
            options: &DecodeOptions,
        ) -> Result<ThumbnailData> {
            let mut processor = Self::new()?;
            processor.open_file(path)?;
            processor.set_decode_options(options);
            let status = unsafe { ffi::libraw_unpack(processor.data) };
            processor.check(status)?;
            let status = unsafe { ffi::libraw_dcraw_process(processor.data) };
            processor.check(status)?;

            let mut error_code = 0;
            let image =
                unsafe { ffi::libraw_dcraw_make_mem_image(processor.data, &mut error_code) };
            processor.copy_image(image, error_code)
        }

        pub(crate) fn version() -> String {
            unsafe {
                let version = ffi::libraw_version();
                if version.is_null() {
                    return "unknown".to_string();
                }
                CStr::from_ptr(version).to_string_lossy().into_owned()
            }
        }

        fn set_decode_options(&mut self, options: &DecodeOptions) {
            unsafe {
                (*self.data).params.half_size = i32::from(options.half_size);
                (*self.data).params.use_camera_wb = i32::from(options.use_camera_wb);
                ffi::libraw_set_demosaic(self.data, options.demosaic_quality);
                ffi::libraw_set_output_bps(self.data, options.output_bps);
                ffi::libraw_set_no_auto_bright(self.data, i32::from(options.no_auto_bright));
                ffi::libraw_set_output_color(self.data, options.output_color);
                if options.linear_gamma {
                    ffi::libraw_set_gamma(self.data, 0, 1.0);
                    ffi::libraw_set_gamma(self.data, 1, 1.0);
                }
            }
        }

        fn copy_image(
            &self,
            image: *mut ffi::libraw_processed_image_t,
            error_code: i32,
        ) -> Result<ThumbnailData> {
            if image.is_null() {
                return Err(self.error(error_code));
            }
            let image_guard = ProcessedImageGuard(image);
            let image = unsafe { &*image_guard.0 };
            let data = unsafe {
                std::slice::from_raw_parts(image.data.as_ptr(), image.data_size as usize).to_vec()
            };
            Ok(ThumbnailData {
                format: ImageFormat::from_code(image.type_ as i32),
                width: image.width,
                height: image.height,
                colors: image.colors,
                bits: image.bits,
                data,
            })
        }

        fn check(&self, status: i32) -> Result<()> {
            if status == ffi::LibRaw_errors_LIBRAW_SUCCESS {
                Ok(())
            } else {
                Err(self.error(status))
            }
        }

        fn error(&self, code: i32) -> RawError {
            let message = unsafe {
                let message = ffi::libraw_strerror(code);
                if message.is_null() {
                    "Unknown LibRaw error".to_string()
                } else {
                    CStr::from_ptr(message).to_string_lossy().into_owned()
                }
            };
            RawError { code, message }
        }
    }

    impl Drop for RawProcessor {
        fn drop(&mut self) {
            if !self.data.is_null() {
                unsafe { ffi::libraw_close(self.data) };
            }
        }
    }

    unsafe impl Send for RawProcessor {}

    struct ProcessedImageGuard(*mut ffi::libraw_processed_image_t);

    impl Drop for ProcessedImageGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { ffi::libraw_dcraw_clear_mem(self.0) };
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) use macos::{DecodeOptions, ImageFormat, RawProcessor};
