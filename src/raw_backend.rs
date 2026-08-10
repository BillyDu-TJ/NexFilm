#[cfg(not(target_os = "macos"))]
mod non_macos {
    pub(crate) use rawlib::{DecodeOptions, ImageFormat, RawProcessor};
    use std::ffi::CStr;
    #[cfg(not(windows))]
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int, c_uchar, c_ushort};
    use std::path::Path;

    #[derive(Debug, Clone)]
    pub(crate) struct CameraRgbData {
        pub(crate) width: u16,
        pub(crate) height: u16,
        pub(crate) colors: u16,
        pub(crate) bits: u16,
        pub(crate) data: Vec<u8>,
        /// Matrix used by LibRaw for camera RGB -> linear sRGB.
        pub(crate) camera_to_srgb: [f32; 9],
    }

    enum LibRawData {}

    #[repr(C)]
    struct LibRawProcessedImage {
        image_type: c_int,
        height: c_ushort,
        width: c_ushort,
        colors: c_ushort,
        bits: c_ushort,
        data_size: u32,
        data: [c_uchar; 1],
    }

    extern "C" {
        fn libraw_init(flags: c_int) -> *mut LibRawData;
        fn libraw_close(data: *mut LibRawData);
        #[cfg(not(windows))]
        fn libraw_open_file(data: *mut LibRawData, path: *const c_char) -> c_int;
        #[cfg(windows)]
        fn libraw_open_wfile(data: *mut LibRawData, path: *const u16) -> c_int;
        fn libraw_unpack(data: *mut LibRawData) -> c_int;
        fn libraw_dcraw_process(data: *mut LibRawData) -> c_int;
        fn libraw_dcraw_make_mem_image(
            data: *mut LibRawData,
            error: *mut c_int,
        ) -> *mut LibRawProcessedImage;
        fn libraw_dcraw_clear_mem(image: *mut LibRawProcessedImage);
        fn libraw_strerror(error: c_int) -> *const c_char;
        fn libraw_set_half_size(data: *mut LibRawData, value: c_int);
        fn libraw_set_use_camera_wb(data: *mut LibRawData, value: c_int);
        fn libraw_set_demosaic(data: *mut LibRawData, value: c_int);
        fn libraw_set_output_bps(data: *mut LibRawData, value: c_int);
        fn libraw_set_no_auto_bright(data: *mut LibRawData, value: c_int);
        fn libraw_set_output_color(data: *mut LibRawData, value: c_int);
        fn libraw_set_gamma(data: *mut LibRawData, index: c_int, value: f32);
        fn libraw_get_rgb_cam(data: *mut LibRawData, row: c_int, column: c_int) -> f32;
    }

    struct Processor(*mut LibRawData);

    impl Drop for Processor {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { libraw_close(self.0) };
            }
        }
    }

    fn error_message(code: c_int) -> String {
        unsafe {
            let message = libraw_strerror(code);
            if message.is_null() {
                format!("LibRaw error {code}")
            } else {
                format!(
                    "LibRaw error {code}: {}",
                    CStr::from_ptr(message).to_string_lossy()
                )
            }
        }
    }

    fn check(code: c_int) -> Result<(), String> {
        if code == 0 {
            Ok(())
        } else {
            Err(error_message(code))
        }
    }

    fn open_file(data: *mut LibRawData, path: &Path) -> Result<(), String> {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            let wide = path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            return check(unsafe { libraw_open_wfile(data, wide.as_ptr()) });
        }
        #[cfg(not(windows))]
        {
            let path = CString::new(path.to_string_lossy().as_bytes())
                .map_err(|error| format!("RAW path contains a null byte: {error}"))?;
            check(unsafe { libraw_open_file(data, path.as_ptr()) })
        }
    }

    /// Decode after LibRaw's black-level, white-balance and demosaic stages,
    /// but before its output-gamut matrix. The latter is applied by the caller
    /// in f32 so signed matrix results are not clipped to unsigned 16-bit.
    pub(crate) fn extract_camera_rgb_with_options<P: AsRef<Path>>(
        path: P,
        options: &DecodeOptions,
    ) -> Result<CameraRgbData, String> {
        let processor = Processor(unsafe { libraw_init(0) });
        if processor.0.is_null() {
            return Err("Failed to initialize LibRaw".to_string());
        }
        open_file(processor.0, path.as_ref())?;
        unsafe {
            libraw_set_half_size(processor.0, i32::from(options.half_size));
            libraw_set_use_camera_wb(processor.0, i32::from(options.use_camera_wb));
            libraw_set_demosaic(processor.0, options.demosaic_quality);
            libraw_set_output_bps(processor.0, options.output_bps);
            libraw_set_no_auto_bright(processor.0, i32::from(options.no_auto_bright));
            libraw_set_output_color(processor.0, 0);
            if options.linear_gamma {
                libraw_set_gamma(processor.0, 0, 1.0);
                libraw_set_gamma(processor.0, 1, 1.0);
            }
        }
        check(unsafe { libraw_unpack(processor.0) })?;
        check(unsafe { libraw_dcraw_process(processor.0) })?;

        let mut camera_to_srgb = [0.0; 9];
        for row in 0..3 {
            for column in 0..3 {
                camera_to_srgb[row * 3 + column] =
                    unsafe { libraw_get_rgb_cam(processor.0, row as c_int, column as c_int) };
            }
        }

        let mut error = 0;
        let image = unsafe { libraw_dcraw_make_mem_image(processor.0, &mut error) };
        if image.is_null() {
            return Err(error_message(error));
        }
        let image_ref = unsafe { &*image };
        let result = CameraRgbData {
            width: image_ref.width,
            height: image_ref.height,
            colors: image_ref.colors,
            bits: image_ref.bits,
            data: unsafe {
                std::slice::from_raw_parts(image_ref.data.as_ptr(), image_ref.data_size as usize)
                    .to_vec()
            },
            camera_to_srgb,
        };
        unsafe { libraw_dcraw_clear_mem(image) };
        Ok(result)
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) use non_macos::{
    extract_camera_rgb_with_options, DecodeOptions, ImageFormat, RawProcessor,
};

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

    #[derive(Debug, Clone)]
    pub(crate) struct CameraRgbData {
        pub(crate) width: u16,
        pub(crate) height: u16,
        pub(crate) colors: u16,
        pub(crate) bits: u16,
        pub(crate) data: Vec<u8>,
        pub(crate) camera_to_srgb: [f32; 9],
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

    pub(crate) fn extract_camera_rgb_with_options<P: AsRef<Path>>(
        path: P,
        options: &DecodeOptions,
    ) -> Result<CameraRgbData> {
        let mut processor = RawProcessor::new()?;
        processor.open_file(path)?;
        let mut camera_options = *options;
        camera_options.output_color = 0;
        processor.set_decode_options(&camera_options);
        let status = unsafe { ffi::libraw_unpack(processor.data) };
        processor.check(status)?;
        let status = unsafe { ffi::libraw_dcraw_process(processor.data) };
        processor.check(status)?;

        let mut camera_to_srgb = [0.0; 9];
        for row in 0..3 {
            for column in 0..3 {
                camera_to_srgb[row * 3 + column] =
                    unsafe { ffi::libraw_get_rgb_cam(processor.data, row as i32, column as i32) };
            }
        }

        let mut error_code = 0;
        let image = unsafe { ffi::libraw_dcraw_make_mem_image(processor.data, &mut error_code) };
        let decoded = processor.copy_image(image, error_code)?;
        Ok(CameraRgbData {
            width: decoded.width,
            height: decoded.height,
            colors: decoded.colors,
            bits: decoded.bits,
            data: decoded.data,
            camera_to_srgb,
        })
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
pub(crate) use macos::{
    extract_camera_rgb_with_options, CameraRgbData, DecodeOptions, ImageFormat, RawProcessor,
};
