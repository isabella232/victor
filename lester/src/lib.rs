use cairo_ffi::*;
use std::error::Error as StdError;
use std::ffi::CStr;
use std::fmt;
use std::io::{self, Read, Write};
use std::mem;
use std::os::raw::*;
use std::slice;

mod cairo_ffi;  // Not public or re-exported

pub struct Argb32Image<'a> {
    pub width: usize,
    pub height: usize,
    pub pixels: &'a mut [u32],
}

pub struct CairoImageSurface {
    ptr: *mut cairo_surface_t,
}

impl Drop for CairoImageSurface {
    fn drop(&mut self) {
        unsafe {
            cairo_surface_destroy(self.ptr);
        }
    }
}

impl CairoImageSurface {
    fn check_status(&self) -> Result<(), CairoError> {
        CairoError::check(unsafe { cairo_surface_status(self.ptr) })
    }

    pub fn as_image(&mut self) -> Argb32Image {
        unsafe {
            let data = cairo_image_surface_get_data(self.ptr);
            let width = cairo_image_surface_get_width(self.ptr);
            let height = cairo_image_surface_get_height(self.ptr);
            let stride = cairo_image_surface_get_stride(self.ptr);
            let format = cairo_image_surface_get_format(self.ptr);
            assert!(format == CAIRO_FORMAT_RGB24 ||
                    format == CAIRO_FORMAT_ARGB32, "Unsupported pixel format");

            // In theory we shouldn’t rely on this.
            // In practice cairo picks a stride that is `width * size_of_pixel`
            // rounded up to 32 bits.
            // ARGB32 and RGB24 both use 32 bit per pixel, so rounding is a no-op.
            assert!(stride == width * (mem::size_of::<u32>() as i32),
                    "Expected 32bit pixel to make width satisfy stride requirements");

            assert!((data as usize) % mem::size_of::<u32>() == 0,
                    "Expected cairo to allocated data aligned to 32 bits");

            // FIXME: checked conversions
            Argb32Image {
                width: width as usize,
                height: height as usize,
                pixels: slice::from_raw_parts_mut(data as *mut u32, (width * height) as usize)
            }
        }
    }
}

macro_rules! with_c_callback {
    (
        $stream: ident : $StreamType: ty : $StreamTrait: ident;
        fn callback($($closure_args: tt)*) -> $ErrorConst: ident $body: block
        ($wrap: expr)($function: ident($($function_args: tt)*))
    ) => {{
        struct ClosureData<Stream> {
            stream: Stream,
            stream_result: Result<(), io::Error>,
        };
        let mut closure_data = ClosureData {
            stream: $stream,
            stream_result: Ok(()),
        };
        let closure_data_ptr: *mut ClosureData<$StreamType> = &mut closure_data;

        unsafe extern "C" fn callback<Stream: $StreamTrait>(
            closure_data_ptr: *mut c_void, $($closure_args)*
        ) -> cairo_status_t {
            // FIXME: catch panics

            let closure_data = &mut *(closure_data_ptr as *mut ClosureData<Stream>);
            if closure_data.stream_result.is_err() {
                return $ErrorConst
            }

            let $stream = &mut closure_data.stream;
            match $body {
                Ok(()) => {
                    CAIRO_STATUS_SUCCESS
                }
                Err(error) => {
                    closure_data.stream_result = Err(error);
                    $ErrorConst
                }
            }
        }

        let result = unsafe {
            $wrap($function(
                $($function_args)*
                callback::<$StreamType>,
                closure_data_ptr as *mut c_void
            ))
        };
        closure_data.stream_result?;
        result
    }}
}


impl CairoImageSurface {
    pub fn read_from_png<R: Read>(stream: R) -> Result<Self, Error> {
        let surface = with_c_callback! {
            stream: R: Read;
            fn callback(buffer: *mut c_uchar, length: c_uint) -> CAIRO_STATUS_WRITE_ERROR {
                // FIXME: checked conversion
                let slice = slice::from_raw_parts_mut(buffer, length as usize);
                stream.read_exact(slice)
            }
            (|ptr| CairoImageSurface { ptr })(cairo_image_surface_create_from_png_stream())
        };

        surface.check_status()?;
        Ok(surface)
    }

    pub fn write_to_png<W: Write>(&self, stream: W) -> Result<(), Error> {
        let status = with_c_callback! {
            stream: W: Write;
            fn callback(buffer: *const c_uchar, length: c_uint) -> CAIRO_STATUS_READ_ERROR {
                // FIXME: checked conversion
                let slice = slice::from_raw_parts(buffer, length as usize);
                stream.write_all(slice)
            }
            (|s| s)(cairo_surface_write_to_png_stream(self.ptr,))
        };

        CairoError::check(status)?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct CairoError {
    status: cairo_status_t,
}

impl CairoError {
    fn check(status: cairo_status_t) -> Result<(), Self> {
        if status == CAIRO_STATUS_SUCCESS {
            Ok(())
        } else {
            Err(CairoError { status })
        }
    }
}

impl StdError for CairoError {
    fn description(&self) -> &str {
        let cstr = unsafe {
            CStr::from_ptr(cairo_status_to_string(self.status))
        };
        cstr.to_str().unwrap()
    }
}

impl fmt::Display for CairoError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.description())
    }
}

impl fmt::Debug for CairoError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.description())
    }
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Cairo(CairoError),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<CairoError> for Error {
    fn from(e: CairoError) -> Self {
        Error::Cairo(e)
    }
}