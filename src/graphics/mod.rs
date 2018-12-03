//! Functions and types used for rendering to the screen.
//!
//! This module implements a (hopefully!) efficent quad renderer, which will queue up
//! drawing operations until it is absolutely necessary to send them to the graphics
//! hardware. This allows us to minimize the number of draw calls made, speeding up
//! rendering.

pub mod animation;
pub mod color;
pub(crate) mod opengl;
pub mod shader;
pub mod texture;

pub use self::animation::Animation;
pub use self::color::Color;
pub use self::shader::Shader;
pub use self::texture::Texture;

use glm::{Mat4, Vec2};
use graphics::opengl::{BufferUsage, GLDevice, GLFramebuffer, GLIndexBuffer, GLVertexBuffer};
use Context;

const SPRITE_CAPACITY: usize = 1024;
const VERTEX_STRIDE: usize = 8;
const INDEX_STRIDE: usize = 6;
const INDEX_ARRAY: [u32; INDEX_STRIDE] = [0, 1, 2, 2, 3, 0];
const DEFAULT_VERTEX_SHADER: &str = include_str!("../resources/shader.vert");
const DEFAULT_FRAGMENT_SHADER: &str = include_str!("../resources/shader.frag");

pub(crate) struct GraphicsContext {
    vertex_buffer: GLVertexBuffer,
    index_buffer: GLIndexBuffer,
    framebuffer: GLFramebuffer,
    framebuffer_texture: Texture,

    texture: Option<Texture>,
    shader: Option<Shader>,
    default_shader: Shader,

    internal_projection: Mat4,
    window_projection: Mat4,

    vertices: Vec<f32>,
    sprite_count: usize,
    capacity: usize,

    internal_width: i32,
    internal_height: i32,
    window_width: i32,
    window_height: i32,
    letterbox: Rectangle,
}

impl GraphicsContext {
    pub(crate) fn new(
        device: &mut GLDevice,
        internal_width: i32,
        internal_height: i32,
        window_width: i32,
        window_height: i32,
    ) -> GraphicsContext {
        assert!(
            SPRITE_CAPACITY <= 8191,
            "Can't have more than 8191 sprites to a single buffer"
        );

        let framebuffer = device.new_framebuffer();
        let framebuffer_texture =
            Texture::from_handle(device.new_texture(internal_width, internal_height));

        device.attach_texture_to_framebuffer(&framebuffer, &framebuffer_texture.handle, false);
        device.set_viewport(0, 0, internal_width, internal_height);

        let indices: Vec<u32> = INDEX_ARRAY
            .iter()
            .cycle()
            .take(SPRITE_CAPACITY * INDEX_STRIDE)
            .enumerate()
            .map(|(i, vertex)| vertex + i as u32 / INDEX_STRIDE as u32 * 4)
            .collect();

        let vertex_buffer = device.new_vertex_buffer(
            SPRITE_CAPACITY * 4 * VERTEX_STRIDE,
            VERTEX_STRIDE,
            BufferUsage::DynamicDraw,
        );

        device.set_vertex_buffer_attribute(&vertex_buffer, 0, 4, 0);
        device.set_vertex_buffer_attribute(&vertex_buffer, 1, 3, 4);

        let index_buffer =
            device.new_index_buffer(SPRITE_CAPACITY * INDEX_STRIDE, BufferUsage::StaticDraw);

        device.set_index_buffer_data(&index_buffer, &indices, 0);

        let default_shader = Shader::from_handle(
            device.compile_program(DEFAULT_VERTEX_SHADER, DEFAULT_FRAGMENT_SHADER),
        );

        GraphicsContext {
            vertex_buffer,
            index_buffer,
            framebuffer,
            framebuffer_texture,

            texture: None,
            shader: None,
            default_shader,

            internal_projection: ortho(
                0.0,
                internal_width as f32,
                internal_height as f32,
                0.0,
                -1.0,
                1.0,
            ),
            window_projection: ortho(
                0.0,
                window_width as f32,
                window_height as f32,
                0.0,
                -1.0,
                1.0,
            ),

            vertices: Vec::with_capacity(SPRITE_CAPACITY * 4 * VERTEX_STRIDE),
            sprite_count: 0,
            capacity: SPRITE_CAPACITY,

            internal_width,
            internal_height,
            window_width,
            window_height,
            letterbox: letterbox(internal_width, internal_height, window_width, window_height),
        }
    }
}

/// A rectangle of `f32`s.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Rectangle {
    /// The X co-ordinate of the rectangle.
    pub x: f32,

    /// The Y co-ordinate of the rectangle.
    pub y: f32,

    /// The width of the rectangle.
    pub width: f32,

    /// The height of the rectangle.
    pub height: f32,
}

impl Rectangle {
    /// Creates a new `Rectangle`.
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Rectangle {
        Rectangle {
            x,
            y,
            width,
            height,
        }
    }

    /// Returns an infinite iterator of horizontally adjecent rectangles, starting at the specified
    /// point and increasing along the X axis.
    ///
    /// This can be useful when slicing spritesheets.
    ///
    /// # Examples
    /// ```
    /// # use tetra::graphics::Rectangle;
    /// let rects: Vec<Rectangle> = Rectangle::row(0.0, 0.0, 16.0, 16.0).take(3).collect();
    ///
    /// assert_eq!(Rectangle::new(0.0, 0.0, 16.0, 16.0), rects[0]);
    /// assert_eq!(Rectangle::new(16.0, 0.0, 16.0, 16.0), rects[1]);
    /// assert_eq!(Rectangle::new(32.0, 0.0, 16.0, 16.0), rects[2]);
    /// ```
    pub fn row(x: f32, y: f32, width: f32, height: f32) -> impl Iterator<Item = Rectangle> {
        RectangleRow {
            next_rect: Rectangle::new(x, y, width, height),
        }
    }

    /// Returns an infinite iterator of vertically adjecent rectangles, starting at the specified
    /// point and increasing along the Y axis.
    ///
    /// This can be useful when slicing spritesheets.
    ///
    /// # Examples
    /// ```
    /// # use tetra::graphics::Rectangle;
    /// let rects: Vec<Rectangle> = Rectangle::column(0.0, 0.0, 16.0, 16.0).take(3).collect();
    ///
    /// assert_eq!(Rectangle::new(0.0, 0.0, 16.0, 16.0), rects[0]);
    /// assert_eq!(Rectangle::new(0.0, 16.0, 16.0, 16.0), rects[1]);
    /// assert_eq!(Rectangle::new(0.0, 32.0, 16.0, 16.0), rects[2]);
    /// ```
    pub fn column(x: f32, y: f32, width: f32, height: f32) -> impl Iterator<Item = Rectangle> {
        RectangleColumn {
            next_rect: Rectangle::new(x, y, width, height),
        }
    }
}

struct RectangleRow {
    next_rect: Rectangle,
}

impl Iterator for RectangleRow {
    type Item = Rectangle;

    fn next(&mut self) -> Option<Rectangle> {
        let current_rect = self.next_rect;
        self.next_rect.x += self.next_rect.width;
        Some(current_rect)
    }
}

struct RectangleColumn {
    next_rect: Rectangle,
}

impl Iterator for RectangleColumn {
    type Item = Rectangle;

    fn next(&mut self) -> Option<Rectangle> {
        let current_rect = self.next_rect;
        self.next_rect.y += self.next_rect.height;
        Some(current_rect)
    }
}

/// Struct representing the parameters that can be used when drawing.
///
/// A default instance of DrawParams will draw the associated graphic with the following
/// settings:
///
/// * Position: [0.0, 0.0]
/// * Scale: [1.0, 1.0]
/// * Origin: [0.0, 0.0]
/// * Color: White
/// * Clip: Full image
pub struct DrawParams {
    pub(crate) position: Vec2,
    pub(crate) scale: Vec2,
    pub(crate) origin: Vec2,
    pub(crate) color: Color,
    pub(crate) clip: Option<Rectangle>,
}

impl DrawParams {
    /// Creates a new set of `DrawParams`.
    pub fn new() -> DrawParams {
        DrawParams::default()
    }

    /// Sets the position that the graphic should be drawn at.
    pub fn position(mut self, position: Vec2) -> DrawParams {
        self.position = position;
        self
    }

    /// Sets the scale that the graphic should be drawn at.
    ///
    /// This can be set to a negative value to flip the graphic around the origin.
    pub fn scale(mut self, scale: Vec2) -> DrawParams {
        self.scale = scale;
        self
    }

    /// Sets the origin of the graphic.
    ///
    /// Positioning and scaling will be calculated relative to this point.
    pub fn origin(mut self, origin: Vec2) -> DrawParams {
        self.origin = origin;
        self
    }

    /// Sets the color to multiply the graphic by.
    ///
    /// Setting this to white will draw the graphic in its original color.
    pub fn color(mut self, color: Color) -> DrawParams {
        self.color = color;
        self
    }

    /// Sets the region of the graphic to draw.
    ///
    /// This is useful if you're using spritesheets (which you should be!).
    pub fn clip(mut self, clip: Rectangle) -> DrawParams {
        self.clip = Some(clip);
        self
    }
}

impl Default for DrawParams {
    fn default() -> DrawParams {
        DrawParams {
            position: Vec2::new(0.0, 0.0),
            scale: Vec2::new(1.0, 1.0),
            origin: Vec2::new(0.0, 0.0),
            color: color::WHITE,
            clip: None,
        }
    }
}

impl From<Vec2> for DrawParams {
    fn from(position: Vec2) -> DrawParams {
        DrawParams {
            position,
            ..DrawParams::default()
        }
    }
}

/// Represents a type that can be drawn to the screen/render target.
///
/// [graphics::draw](fn.draw.html) can be used to draw without importing this trait, which is sometimes
/// more convienent.
pub trait Drawable {
    /// Draws `self` to the currently enabled render target, using the specified parameters.
    ///
    /// Any type that implements `Into<DrawParams>` can be passed into this method. For example, since the majority
    /// of the time, you only care about changing the position, a `Vec2` can be passed to set the position and leave
    /// everything else as the defaults.
    fn draw<T: Into<DrawParams>>(&self, ctx: &mut Context, params: T);
}

/// Gets the internal width of the screen, before scaling is applied.
pub fn get_width(ctx: &Context) -> i32 {
    ctx.graphics.internal_width
}

/// Gets the internal height of the screen, before scaling is applied.
pub fn get_height(ctx: &Context) -> i32 {
    ctx.graphics.internal_height
}

/// Gets the width of the window.
pub fn get_window_width(ctx: &Context) -> i32 {
    ctx.graphics.window_width
}

/// Gets the height of the window.
pub fn get_window_height(ctx: &Context) -> i32 {
    ctx.graphics.window_height
}

pub(crate) fn get_letterbox(ctx: &Context) -> Rectangle {
    ctx.graphics.letterbox
}

/// Clears the currently enabled render target to the specified color.
pub fn clear(ctx: &mut Context, color: Color) {
    ctx.gl.clear(color.r, color.g, color.b, color.a);
}

pub(crate) fn push_vertex(ctx: &mut Context, x: f32, y: f32, u: f32, v: f32, color: Color) {
    ctx.graphics.vertices.push(x);
    ctx.graphics.vertices.push(y);
    ctx.graphics.vertices.push(u);
    ctx.graphics.vertices.push(v);
    ctx.graphics.vertices.push(color.r);
    ctx.graphics.vertices.push(color.g);
    ctx.graphics.vertices.push(color.b);
    ctx.graphics.vertices.push(color.a);
}

/// Draws an object to the currently enabled render target.
///
/// This function simply calls [`draw`](trait.Drawable.html#tymethod.draw) on the passed object - it is
/// provided to allow you to avoid having to import the [`Drawable`](trait.Drawable.html) trait as well
/// as the `graphics` module.
pub fn draw<D: Drawable, P: Into<DrawParams>>(ctx: &mut Context, drawable: &D, params: P) {
    drawable.draw(ctx, params);
}

/// Sets the texture that is currently being used for rendering.
///
/// If the texture is different from the one that is currently in use, this will trigger a
/// [`flush`](fn.flush.html) to the graphics hardware - try to avoid texture swapping as
/// much as you can.
pub fn set_texture(ctx: &mut Context, texture: &Texture) {
    match ctx.graphics.texture {
        Some(ref inner) if inner == texture => {}
        None => {
            ctx.graphics.texture = Some(texture.clone());
        }
        _ => {
            ctx.graphics.texture = Some(texture.clone());
            flush(ctx);
        }
    }
}

/// Sends queued data to the graphics hardware.
///
/// You usually will not have to call this manually, as [`set_texture`](fn.set_texture.html) and
/// [`present`](fn.present.html) will automatically flush when necessary. Try to keep flushing
/// to a minimum, as this will reduce the number of draw calls made to the graphics device.
pub fn flush(ctx: &mut Context) {
    if ctx.graphics.sprite_count > 0 && ctx.graphics.texture.is_some() {
        let texture = ctx.graphics.texture.as_ref().unwrap();
        let shader = ctx
            .graphics
            .shader
            .as_ref()
            .unwrap_or(&ctx.graphics.default_shader);

        ctx.gl.set_uniform(
            &shader.handle,
            "projection",
            &ctx.graphics.internal_projection,
        );

        ctx.gl
            .set_vertex_buffer_data(&ctx.graphics.vertex_buffer, &ctx.graphics.vertices, 0);

        ctx.gl.draw(
            &ctx.graphics.vertex_buffer,
            &ctx.graphics.index_buffer,
            &shader.handle,
            &texture.handle,
            ctx.graphics.sprite_count * INDEX_STRIDE,
        );

        ctx.graphics.vertices.clear();
        ctx.graphics.sprite_count = 0;
    }
}

/// Draws the currently enabled render target to the screen, scaling/letterboxing it if necessary.
///
/// You usually will not have to call this manually, as it is called for you at the end of every
/// frame. Note that calling it will trigger a [`flush`](fn.flush.html) to the graphics hardware.
pub fn present(ctx: &mut Context) {
    flush(ctx);

    ctx.gl.bind_default_framebuffer();
    ctx.gl
        .set_viewport(0, 0, ctx.graphics.window_width, ctx.graphics.window_height);
    clear(ctx, color::BLACK);

    let letterbox = ctx.graphics.letterbox;

    push_vertex(ctx, letterbox.x, letterbox.y, 0.0, 1.0, color::WHITE);

    push_vertex(
        ctx,
        letterbox.x,
        letterbox.y + letterbox.height,
        0.0,
        0.0,
        color::WHITE,
    );

    push_vertex(
        ctx,
        letterbox.x + letterbox.width,
        letterbox.y + letterbox.height,
        1.0,
        0.0,
        color::WHITE,
    );

    push_vertex(
        ctx,
        letterbox.x + letterbox.width,
        letterbox.y,
        1.0,
        1.0,
        color::WHITE,
    );

    ctx.gl.set_uniform(
        &ctx.graphics.default_shader.handle,
        "projection",
        &ctx.graphics.window_projection,
    );

    ctx.gl
        .set_vertex_buffer_data(&ctx.graphics.vertex_buffer, &ctx.graphics.vertices, 0);

    ctx.gl.draw(
        &ctx.graphics.vertex_buffer,
        &ctx.graphics.index_buffer,
        &ctx.graphics.default_shader.handle,
        &ctx.graphics.framebuffer_texture.handle,
        INDEX_STRIDE,
    );

    ctx.graphics.vertices.clear();

    ctx.window.gl_swap_window();

    ctx.gl.bind_framebuffer(&ctx.graphics.framebuffer);
    ctx.gl.set_viewport(
        0,
        0,
        ctx.graphics.internal_width,
        ctx.graphics.internal_height,
    );
}

pub(crate) fn set_window_size(ctx: &mut Context, width: i32, height: i32) {
    ctx.graphics.window_width = width;
    ctx.graphics.window_height = height;
    ctx.graphics.window_projection = ortho(0.0, width as f32, height as f32, 0.0, -1.0, 1.0);
    ctx.graphics.letterbox = letterbox(
        ctx.graphics.internal_width,
        ctx.graphics.internal_height,
        width,
        height,
    );
}

fn letterbox(
    internal_width: i32,
    internal_height: i32,
    window_width: i32,
    window_height: i32,
) -> Rectangle {
    let scale_factor = if window_width <= window_height {
        window_width / internal_width
    } else {
        window_height / internal_height
    };

    let letterbox_width = internal_width * scale_factor;
    let letterbox_height = internal_height * scale_factor;
    let letterbox_x = (window_width - letterbox_width) / 2;
    let letterbox_y = (window_height - letterbox_height) / 2;

    Rectangle::new(
        letterbox_x as f32,
        letterbox_y as f32,
        letterbox_width as f32,
        letterbox_height as f32,
    )
}

pub(crate) fn ortho(left: f32, right: f32, bottom: f32, top: f32, near: f32, far: f32) -> Mat4 {
    // Taken from GGEZ - nalgebra doesn't like upside-down projections
    let c0r0 = 2.0 / (right - left);
    let c0r1 = 0.0;
    let c0r2 = 0.0;
    let c0r3 = 0.0;
    let c1r0 = 0.0;
    let c1r1 = 2.0 / (top - bottom);
    let c1r2 = 0.0;
    let c1r3 = 0.0;
    let c2r0 = 0.0;
    let c2r1 = 0.0;
    let c2r2 = -2.0 / (far - near);
    let c2r3 = 0.0;
    let c3r0 = -(right + left) / (right - left);
    let c3r1 = -(top + bottom) / (top - bottom);
    let c3r2 = -(far + near) / (far - near);
    let c3r3 = 1.0;

    Mat4::from([
        [c0r0, c0r1, c0r2, c0r3],
        [c1r0, c1r1, c1r2, c1r3],
        [c2r0, c2r1, c2r2, c2r3],
        [c3r0, c3r1, c3r2, c3r3],
    ])
}
