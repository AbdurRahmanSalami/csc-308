mod constants;
use core::{
    fmt::{self, Write},
    ptr,
};
use bootloader_api::info::{FrameBufferInfo, PixelFormat};
use constants::font_constants;
use constants::font_constants::{BACKUP_CHAR, CHAR_RASTER_HEIGHT, FONT_WEIGHT};
use noto_sans_mono_bitmap::{get_raster, RasterizedChar};

/// Additional vertical space between lines
const LINE_SPACING: usize = 2;

/// Additional horizontal space between characters.
const LETTER_SPACING: usize = 0;

/// Padding from the border. Prevent that font is too close to border.
const BORDER_PADDING: usize = 1;

// ANSI-like color codes
const COLOR_BLUE: [u8; 3] = [255, 0, 0]; // RGB for blue
const COLOR_WHITE: [u8; 3] = [255, 255, 255]; // RGB for white (default color)

/// Returns the raster of the given char or the raster of [font_constants::BACKUP_CHAR].
fn get_char_raster(c: char) -> RasterizedChar {
    fn get(c: char) -> Option<RasterizedChar> {
        get_raster(c, FONT_WEIGHT, CHAR_RASTER_HEIGHT)
    }
    get(c).unwrap_or_else(|| get(BACKUP_CHAR).expect("Should get raster of backup char."))
}

#[macro_export]
macro_rules! print {
    ($writer:expr, $($arg:tt)*) => {{
        use core::fmt::Write;
        let _ = write!($writer, $($arg)*);
    }};
}

/// Allows logging text to a pixel-based framebuffer.
pub struct FrameBufferWriter {
    framebuffer: &'static mut [u8],
    info: FrameBufferInfo,
    x_pos: usize,
    y_pos: usize,
    current_color: [u8; 3],
}

impl FrameBufferWriter {
    /// Creates a new logger that uses the given framebuffer.
    pub fn new(framebuffer: &'static mut [u8], info: FrameBufferInfo) -> Self {
        let mut logger = Self {
            framebuffer,
            info,
            x_pos: BORDER_PADDING,
            y_pos: BORDER_PADDING,
            current_color: COLOR_WHITE,
        };
        logger.clear();
        logger
    }

    /// Moves the cursor to the next line. Handles vertical overflow by scrolling the screen.
    fn newline(&mut self) {
        self.y_pos += font_constants::CHAR_RASTER_HEIGHT.val() + LINE_SPACING;
        if self.y_pos >= self.height() {
            self.scroll_screen();
        }
        self.carriage_return();
    }

    /// Moves the cursor to the beginning of the current line.
    fn carriage_return(&mut self) {
        self.x_pos = BORDER_PADDING;
    }

    /// Scrolls the screen up by one line when vertical overflow occurs.
    fn scroll_screen(&mut self) {
        let line_height = font_constants::CHAR_RASTER_HEIGHT.val() + LINE_SPACING;
        let bytes_per_line = self.info.stride * line_height * self.info.bytes_per_pixel;
        let screen_size = self.framebuffer.len();

        // Move all lines up by one line
        self.framebuffer.copy_within(bytes_per_line..screen_size, 0);

        // Clear the last line
        let last_line_start = screen_size - bytes_per_line;
        self.framebuffer[last_line_start..].fill(0);

        // Adjust the y position
        self.y_pos -= line_height;
    }

    /// Erases all text on the screen. Resets self.x_pos and self.y_pos.
    pub fn clear(&mut self) {
        self.x_pos = BORDER_PADDING;
        self.y_pos = BORDER_PADDING;
        self.framebuffer.fill(0);
    }

    /// Returns the width of the framebuffer.
    fn width(&self) -> usize {
        self.info.width
    }

    /// Returns the height of the framebuffer.
    fn height(&self) -> usize {
        self.info.height
    }

    /// Writes a single char to the framebuffer. Takes care of special control characters, such as
    /// newlines and carriage returns.
    fn write_char(&mut self, c: char) {
        match c {
            '\n' => self.newline(),
            '\r' => self.carriage_return(),
            '\t' => {
                let tab_size = 4; // Number of spaces for a tab
                for _ in 0..tab_size {
                    self.write_char(' ');
                }
            },
            c => {
                let new_xpos = self.x_pos + font_constants::CHAR_RASTER_WIDTH;
                if new_xpos >= self.width() {
                    self.newline();
                }
                let new_ypos = self.y_pos + font_constants::CHAR_RASTER_HEIGHT.val() + BORDER_PADDING;
                if new_ypos >= self.height() {
                    self.scroll_screen();
                }
                self.write_rendered_char(get_char_raster(c));
            }
        }
    }

    /// Prints a rendered char into the framebuffer.
    /// Updates self.x_pos.
    fn write_rendered_char(&mut self, rendered_char: RasterizedChar) {
        for (y, row) in rendered_char.raster().iter().enumerate() {
            for (x, byte) in row.iter().enumerate() {
                if *byte > 0 {
                    self.write_pixel(self.x_pos + x, self.y_pos + y, *byte);
                }
            }
        }
        self.x_pos += rendered_char.width() + LETTER_SPACING;
    }

    /// Writes a pixel to the framebuffer at the specified position.
    fn write_pixel(&mut self, x: usize, y: usize, intensity: u8) {
        let pixel_offset = y * self.info.stride + x;
        let color = [
            (self.current_color[0] as u16 * intensity as u16 / 255) as u8,
            (self.current_color[1] as u16 * intensity as u16 / 255) as u8,
            (self.current_color[2] as u16 * intensity as u16 / 255) as u8,
        ];
        let bytes_per_pixel = self.info.bytes_per_pixel;
        let byte_offset = pixel_offset * bytes_per_pixel;
        self.framebuffer[byte_offset..(byte_offset + bytes_per_pixel)]
            .copy_from_slice(&color[..bytes_per_pixel]);
        let _ = unsafe { ptr::read_volatile(&self.framebuffer[byte_offset]) };
    }

    /// Prints text with automatic wrapping, scrolling, and ANSI-like escape sequences.
    pub fn print(&mut self, text: &str) {
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    if let Some(next) = chars.next() {
                        match next {
                            'c' => self.current_color = COLOR_BLUE,  // Change to blue
                            'r' => self.current_color = COLOR_WHITE, // Reset to white
                            _ => self.write_char(c),                // Unknown sequence
                        }
                    }
                }
                _ => self.write_char(c),
            }
        }
    }
}

unsafe impl Send for FrameBufferWriter {}
unsafe impl Sync for FrameBufferWriter {}

impl Write for FrameBufferWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            self.write_char(c);
        }
        Ok(())
    }
}