use cairo_ffi::*;
use errors::{CairoError, CairoOrIoError};
use std::any::Any;
use std::fs;
use std::io::{self, Read, Write};
use std::mem;
use std::os::raw::*;
use std::panic;
use std::path;
use std::slice;

macro_rules! antialias {
    ($( $Variant: ident => $constant: expr, )+) => {
        /// A cairo antialiasing mode.
        ///
        /// See [`cairo_antialias_t`] for the meaning of each value.
        ///
        /// [`cairo_antialias_t`]: https://www.cairographics.org/manual/cairo-cairo-t.html#cairo-antialias-t
        #[derive(Copy, Clone, Debug, Eq, PartialEq)]
        pub enum Antialias {
            $(
                $Variant,
            )+
        }

        impl Antialias {
            pub(crate) fn to_cairo(&self) -> cairo_antialias_t {
                match *self {
                    $(
                        Antialias::$Variant => $constant,
                    )+
                }
            }
        }
    }
}

antialias! {
    Default => CAIRO_ANTIALIAS_DEFAULT,
    None => CAIRO_ANTIALIAS_NONE,
    Gray => CAIRO_ANTIALIAS_GRAY,
    Subpixel => CAIRO_ANTIALIAS_SUBPIXEL,
    Fast => CAIRO_ANTIALIAS_FAST,
    Good => CAIRO_ANTIALIAS_GOOD,
    Best => CAIRO_ANTIALIAS_BEST,
}

/// The pixels from an `ImageSurface`
pub struct Argb32Image<'data> {
    pub width: usize,
    pub height: usize,
    pub pixels: &'data mut [u32],
}

/// A cairo “image surface”: an in-memory pixel buffer.
///
/// Only the RGB24 and ARGB32 pixel formats (which have compatible memory representation)
/// are supported.
pub struct ImageSurface {
    pub(crate) ptr: *mut cairo_surface_t,
}

impl Drop for ImageSurface {
    fn drop(&mut self) {
        unsafe {
            cairo_surface_destroy(self.ptr);
        }
    }
}

impl ImageSurface {
    /// Create a new RGB24 image surface of the given size, in pixels
    pub fn new_rgb24(width: usize, height: usize) -> Result<Self, CairoError> {
        Self::new(CAIRO_FORMAT_RGB24, width, height)
    }

    /// Create a new ARGB32 image surface of the given size, in pixels
    pub fn new_argb32(width: usize, height: usize) -> Result<Self, CairoError> {
        Self::new(CAIRO_FORMAT_ARGB32, width, height)
    }

    fn new(format: cairo_format_t, width: usize, height: usize) -> Result<Self, CairoError> {
        unsafe {
            let ptr = cairo_image_surface_create(format, width as _, height as _);
            let surface = ImageSurface { ptr };
            surface.check_status()?;
            Ok(surface)
        }
    }

    fn check_status(&self) -> Result<(), CairoError> {
        CairoError::check(unsafe { cairo_surface_status(self.ptr) })
    }

    pub(crate) fn context(&self) -> Result<CairoContext, CairoError> {
        unsafe {
            let context = CairoContext { ptr: cairo_create(self.ptr) };
            context.check_status()?;
            Ok(context)
        }
    }

    /// Access the pixels of this image surface
    pub fn as_image<'data>(&'data mut self) -> Argb32Image<'data> {
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

    /// Read and decode a PNG image from the given file name and create an image surface for it.
    pub fn read_from_png_file<P: AsRef<path::Path>>(filename: P) -> Result<Self, CairoOrIoError> {
        Self::read_from_png(io::BufReader::new(fs::File::open(filename)?))
    }

    /// Encode this image to PNG and write it into the file with the given name.
    pub fn write_to_png_file<P: AsRef<path::Path>>(&self, filename: P) -> Result<(), CairoOrIoError> {
        self.write_to_png(io::BufWriter::new(fs::File::create(filename)?))
    }
}

// Private
pub(crate) struct CairoContext {
    pub(crate) ptr: *mut cairo_t,
}

impl CairoContext {
    pub(crate) fn check_status(&self) -> Result<(), CairoError> {
        CairoError::check(unsafe { cairo_status(self.ptr) })
    }
}

impl Drop for CairoContext {
    fn drop(&mut self) {
        unsafe {
            cairo_destroy(self.ptr);
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
            panic_payload: Option<Box<Any + Send + 'static>>
        };
        let mut closure_data = ClosureData {
            stream: $stream,
            stream_result: Ok(()),
            panic_payload: None,
        };
        let closure_data_ptr: *mut ClosureData<$StreamType> = &mut closure_data;

        unsafe extern "C" fn callback<Stream: $StreamTrait>(
            closure_data_ptr: *mut c_void, $($closure_args)*
        ) -> cairo_status_t {
            let panic_result = panic::catch_unwind(|| {
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
            });
            match panic_result {
                Ok(value) => value,
                Err(panic_payload) => {
                    let closure_data = &mut *(closure_data_ptr as *mut ClosureData<Stream>);
                    closure_data.panic_payload = Some(panic_payload);
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
        if let Some(panic_payload) = closure_data.panic_payload {
            panic::resume_unwind(panic_payload)
        }
        closure_data.stream_result?;
        result
    }}
}


impl ImageSurface {
    /// Read and decode a PNG image from the given stream and create an image surface for it.
    ///
    /// Note: this may do many read calls.
    /// If a stream is backed by costly system calls (such as `File` or `TcpStream`),
    /// this constructor will likely perform better with that stream wrapped in `BufReader`.
    pub fn read_from_png<R: Read>(stream: R) -> Result<Self, CairoOrIoError> {
        let surface = with_c_callback! {
            stream: R: Read;
            fn callback(buffer: *mut c_uchar, length: c_uint) -> CAIRO_STATUS_WRITE_ERROR {
                // FIXME: checked conversion
                let slice = slice::from_raw_parts_mut(buffer, length as usize);
                stream.read_exact(slice)
            }
            (|ptr| ImageSurface { ptr })(cairo_image_surface_create_from_png_stream())
        };

        surface.check_status()?;
        Ok(surface)
    }

    /// Encode this image to PNG and write it to the given stream.
    ///
    /// Note: this may do many read calls.
    /// If a stream is backed by costly system calls (such as `File` or `TcpStream`),
    /// this constructor will likely perform better with that stream wrapped in `BufWriter`.
    ///
    /// See also the `write_to_png_file` method.
    pub fn write_to_png<W: Write>(&self, stream: W) -> Result<(), CairoOrIoError> {
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
