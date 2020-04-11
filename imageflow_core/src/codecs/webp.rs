use std;
use for_other_imageflow_crates::preludes::external_without_std::*;
use ffi;
use ::{Context, CError,  Result, JsonResponse};
use ffi::CodecInstance;
use ffi::BitmapBgra;
use imageflow_types::collections::AddRemoveSet;
use io::IoProxy;
use uuid::Uuid;
use imageflow_types::IoDirection;
use super::*;
use std::any::Any;
use gif::Frame;
use gif::SetParameter;
use std::rc::Rc;
use io::IoProxyProxy;
use io::IoProxyRef;
use rgb::alt::BGRA8;
use libwebp_sys::*;
use libwebp_sys::WEBP_CSP_MODE::MODE_BGRA;
use imageflow_helpers::preludes::from_std::ptr::null;


pub struct WebPDecoder{
    io:  IoProxy,
    bytes: Option<Vec<u8>>,
    config: Option<WebPDecoderConfig>,
    features_read: bool
}

impl WebPDecoder {
    pub fn create(c: &Context, io: IoProxy, io_id: i32) -> Result<WebPDecoder> {
        Ok(WebPDecoder{
            io,
            bytes: None,
            config: None,
            features_read: false
        })
    }

    pub fn ensure_data_buffered(&mut self, c: &Context) -> Result<()>{
        if self.bytes.is_none() {
            let mut bytes = Vec::with_capacity(2048);
            let _ = self.io.read_to_end(&mut bytes).map_err(|e| FlowError::from_decoder(e));
            self.bytes = Some(bytes);
        }
        Ok(())
    }

    pub fn input_width(&self) -> Option<i32>{
        if self.features_read {
            Some(self.config.unwrap().input.width)
        }else{
            None
        }
    }
    pub fn input_height(&self) -> Option<i32>{
        if self.features_read {
            Some(self.config.unwrap().input.height)
        }else{
            None
        }
    }
    pub fn output_width(&self) -> Option<i32>{
        if self.features_read && self.config.unwrap().options.use_scaling == 1{
            Some(self.config.unwrap().options.scaled_width)
        }else{
            self.input_width()
        }
    }
    pub fn output_height(&self) -> Option<i32>{
        if self.features_read && self.config.unwrap().options.use_scaling == 1{
            Some(self.config.unwrap().options.scaled_height)
        }else{
            self.input_height()
        }
    }
}


impl Decoder for WebPDecoder {
    fn initialize(&mut self, c: &Context) -> Result<()> {
        self.config = Some(WebPDecoderConfig::new()
            .expect("Failed to initialize WebPDecoderConfig"));

        Ok(())
    }


    fn get_image_info(&mut self, c: &Context) -> Result<s::ImageInfo> {
        self.ensure_data_buffered(c)?;
        if !self.features_read {
            let buf = &self.bytes.as_ref().unwrap(); //Cannot fail after ensure_data_buffered
            let len = buf.len();
            unsafe {
                let error_code = WebPGetFeatures(buf.as_ptr(), len, &mut self.config.unwrap().input);
                if error_code != VP8StatusCode::VP8_STATUS_OK {
                    return Err(nerror!(ErrorKind::ImageDecodingError, "libwebp features decoding error {:?}", error_code));
                }
            }
        }
        Ok(s::ImageInfo {
            frame_decodes_into: s::PixelFormat::Bgra32,
            image_width: self.input_width().unwrap(),
            image_height: self.input_height().unwrap(),
            preferred_mime_type: "image/webp".to_owned(),
            preferred_extension: "webp".to_owned()
        })
    }

    //Webp ignores exif rotation in Chrome, so we ignore it
    fn get_exif_rotation_flag(&mut self, c: &Context) -> Result<Option<i32>> {
        Ok(None)
    }

    fn tell_decoder(&mut self, c: &Context, tell: s::DecoderCommand) -> Result<()> {
        if let s::DecoderCommand::WebPDecoderHints(hints) = tell{
            self.config.unwrap().options.use_scaling = 1;
            self.config.unwrap().options.scaled_width = hints.width;
            self.config.unwrap().options.scaled_height = hints.height;
        }
        Ok(())
    }

    fn read_frame(&mut self, c: &Context) -> Result<*mut BitmapBgra> {
        let _ = self.get_image_info(c)?;

        unsafe {
            let w = self.output_width().unwrap();
            let h = self.output_height().unwrap();
            let copy = ffi::flow_bitmap_bgra_create(c.flow_c(), w as i32, h as i32, false, ffi::PixelFormat::Bgra32);
            if copy.is_null() {
                cerror!(c).panic();
            }


            // Specify the desired output colorspace:
            self.config.unwrap().output.colorspace = MODE_BGRA;
            // Have config.output point to an external buffer:
            self.config.unwrap().output.u.RGBA.rgba = (*copy).pixels;
            self.config.unwrap().output.u.RGBA.stride = (*copy).stride as i32;
            self.config.unwrap().output.u.RGBA.size = (*copy).stride as usize * (*copy).h as usize;
            self.config.unwrap().output.is_external_memory = 1;


            let len = self.bytes.as_ref().unwrap().len();

            let error_code = WebPDecode(self.bytes.as_ref().unwrap().as_ptr(), len, &mut self.config.unwrap());
            if error_code != VP8StatusCode::VP8_STATUS_OK {
                return Err(nerror!(ErrorKind::ImageDecodingError, "libwebp decoding error {:?}", error_code));
            }

            Ok(copy)
        }
    }
    fn has_more_frames(&mut self) -> Result<bool> {
        Ok(false) // TODO: support webp animation
    }
    fn as_any(&self) -> &dyn Any {
        self as &dyn Any
    }
}


pub struct WebPEncoder {
    io: IoProxy
}

impl WebPEncoder {
    pub(crate) fn create(c: &Context, io: IoProxy) -> Result<Self> {
        Ok(WebPEncoder {
            io
        })
    }
}

impl Encoder for WebPEncoder {
    fn write_frame(&mut self, c: &Context, preset: &s::EncoderPreset, frame: &mut BitmapBgra, decoder_io_ids: &[i32]) -> Result<s::EncodeResult> {

        unsafe {
            let mut output: *mut u8 = ptr::null_mut();
            let output_len: usize;

            match preset {
                s::EncoderPreset::WebPLossy { quality } => {
                    output_len = WebPEncodeBGRA(frame.pixels, frame.width() as i32, frame.height() as i32, frame.stride() as i32, *quality, &mut output);
                },
                s::EncoderPreset::WebPLossless => {
                    output_len = WebPEncodeLosslessBGRA(frame.pixels, frame.width() as i32, frame.height() as i32, frame.stride() as i32, &mut output);
                },
                _ => {
                    panic!("Incorrect encoder for encoder preset")
                }
            }
            if output_len == 0 {
                return Err(nerror!(ErrorKind::ImageEncodingError, "libwebp encoding error"));
            } else {
                let bytes = slice::from_raw_parts(output, output_len);
                self.io.write_all(bytes).map_err(|e| FlowError::from_encoder(e).at(here!()))?;
                WebPFree(output as *mut libc::c_void);
            }
        }

        Ok(s::EncodeResult {
            w: frame.w as i32,
            h: frame.h as i32,
            io_id: self.io.io_id(),
            bytes: ::imageflow_types::ResultBytes::Elsewhere,
            preferred_extension: "webp".to_owned(),
            preferred_mime_type: "image/webp".to_owned(),
        })
    }

    fn get_io(&self) -> Result<IoProxyRef> {
        Ok(IoProxyRef::Borrow(&self.io))
    }
}
