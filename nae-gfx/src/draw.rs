use nae_core::{
    BaseGfx, BasePipeline, BlendMode, ClearOptions, Color, DrawUsage, Geometry, GraphicsAPI,
    PipelineOptions,
};

use crate::batchers::{ColorBatcher, ImageBatcher, PatternBatcher};
use crate::shapes::ShapeTessellator;
use crate::texture::Texture;
use crate::{
    matrix4_identity, matrix4_mul_matrix4, matrix4_mul_vector4, matrix4_orthogonal, Device,
    Graphics, IndexBuffer, Matrix4, Pipeline, Shader, Uniform, VertexAttr, VertexBuffer,
    VertexFormat,
};
use std::cell::RefMut;
use std::convert::TryInto;

pub struct Draw {
    pub gfx: Graphics,
    pub depth: f32,
    pub color: Color,
    pub alpha: f32,
    pub blend_mode: Option<BlendMode>,
    pub projection: Option<Matrix4>,
    pub matrix: Option<Matrix4>,

    render_projection: Matrix4,
    matrix_stack: Vec<Matrix4>,
    clear_options: ClearOptions,
    color_batcher: ColorBatcher,
    image_batcher: ImageBatcher,
    pattern_batcher: PatternBatcher,
    current_mode: PaintMode,
    shapes: ShapeTessellator,
}

impl Draw {
    pub fn new(device: &Device) -> Result<Self, String> {
        let mut gfx = Graphics::new(device)?;
        let color_batcher = ColorBatcher::new(&mut gfx)?;
        let image_batcher = ImageBatcher::new(&mut gfx)?;
        let pattern_batcher = PatternBatcher::new(&mut gfx)?;

        let (width, height) = gfx.size(); //TODO multiply for dpi
        let render_projection = matrix4_orthogonal(0.0, width, height, 0.0, -1.0, 1.0);

        Ok(Self {
            gfx,
            clear_options: Default::default(),
            color: Color::WHITE,
            alpha: 1.0,
            depth: 0.0,
            blend_mode: Some(BlendMode::NORMAL),
            current_mode: PaintMode::None,
            matrix_stack: vec![matrix4_identity()],
            color_batcher,
            image_batcher,
            pattern_batcher,
            matrix: None,
            projection: None,
            render_projection,
            shapes: ShapeTessellator::new(),
        })
    }

    pub fn set_size(&mut self, width: f32, height: f32) {
        self.gfx.set_size(width, height);
        self.render_projection = matrix4_orthogonal(0.0, width, height, 0.0, -1.0, 1.0);
    }

    pub fn size(&self) -> (f32, f32) {
        self.gfx.size()
    }

    pub fn push(&mut self, matrix: &Matrix4) {
        let new_matrix = matrix4_mul_matrix4(self.transform(), matrix);
        self.matrix_stack.push(new_matrix);
    }

    pub fn pop(&mut self) {
        if self.matrix_stack.len() <= 1 {
            return;
        }

        self.matrix_stack.pop();
    }

    pub fn transform(&mut self) -> &Matrix4 {
        self.matrix_stack.last().as_ref().unwrap()
    }

    pub fn begin(&mut self, color: Color) {
        self.clear_options.color = Some(color);
        self.gfx.begin(&self.clear_options);
    }

    pub fn end(&mut self) {
        paint_mode(self, PaintMode::None);
        self.gfx.end();
    }

    pub fn geometry(&mut self, geometry: &Geometry) {
        paint_mode(self, PaintMode::Color);
        geometry.data().iter().for_each(|data| {
            draw_color(self, &data.vertices, &data.indices, Some(data.color));
        });
    }

    pub fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, width: f32) {
        paint_mode(self, PaintMode::Color);

        let (mut xx, mut yy) = if y1 == y2 {
            (0.0, -1.0)
        } else {
            (1.0, -(x2 - x1) / (y2 - y1))
        };

        let len = (xx * xx + yy * yy).sqrt();
        if len != 0.0 {
            let mul = width / len;
            xx *= mul;
            yy *= mul;
        }

        let px1 = x1 + 0.5 * xx;
        let py1 = y1 + 0.5 * yy;
        let px2 = x2 + 0.5 * xx;
        let py2 = y2 + 0.5 * yy;
        let px3 = px1 - xx;
        let py3 = py1 - yy;
        let px4 = px2 - xx;
        let py4 = py2 - yy;

        #[rustfmt::skip]
            draw_color(
            self,
            &[
                px1, py1, self.depth,
                px2, py2, self.depth,
                px3, py3, self.depth,
                px3, py3, self.depth,
                px2, py2, self.depth,
                px4, py4, self.depth
            ],
            &[
                0, 1, 2, 3, 4, 5
            ],
            None,
        );
    }

    pub fn stroke_triangle(
        &mut self,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        x3: f32,
        y3: f32,
        line_width: f32,
    ) {
        paint_mode(self, PaintMode::Color);
        let (vertices, indices) = self
            .shapes
            .stroke_triangle(x1, y1, x2, y2, x3, y3, line_width, self.depth);

        draw_color(self, &vertices, &indices, None);
    }

    pub fn triangle(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x3: f32, y3: f32) {
        paint_mode(self, PaintMode::Color);

        #[rustfmt::skip]
            draw_color(
            self,
            &[
                x1, y1, self.depth,
                x2, y2, self.depth,
                x3, y3, self.depth
            ],
            &[0, 1, 2],
            None,
        );
    }

    pub fn stroke_rect(&mut self, x: f32, y: f32, width: f32, height: f32, line_width: f32) {
        paint_mode(self, PaintMode::Color);
        let (vertices, indices) = self
            .shapes
            .stroke_rect(x, y, width, height, line_width, self.depth);

        draw_color(self, &vertices, &indices, None);
    }

    pub fn rect(&mut self, x: f32, y: f32, width: f32, height: f32) {
        paint_mode(self, PaintMode::Color);

        let x2 = x + width;
        let y2 = y + height;

        #[rustfmt::skip]
            draw_color(
            self,
            &[
                x, y, self.depth,
                x2, y, self.depth,
                x, y2, self.depth,
                x2, y2, self.depth,
            ],
            &[0, 1, 2, 2, 1, 3],
            None
        );
    }

    pub fn stroke_circle(&mut self, x: f32, y: f32, radius: f32, line_width: f32) {
        paint_mode(self, PaintMode::Color);
        let (vertices, indices) = self
            .shapes
            .stroke_circle(x, y, radius, line_width, self.depth);

        draw_color(self, &vertices, &indices, None);
    }

    pub fn circle(&mut self, x: f32, y: f32, radius: f32) {
        paint_mode(self, PaintMode::Color);
        let (vertices, indices) = self.shapes.circle(x, y, radius, self.depth);

        draw_color(self, &vertices, &indices, None);
    }

    pub fn rounded_rect(&mut self, x: f32, y: f32, width: f32, height: f32, corner_radius: f32) {
        paint_mode(self, PaintMode::Color);
        let (vertices, indices) =
            self.shapes
                .rounded_rect(x, y, width, height, corner_radius, self.depth);

        draw_color(self, &vertices, &indices, None);
    }

    pub fn stroke_rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        corner_radius: f32,
        line_width: f32,
    ) {
        paint_mode(self, PaintMode::Color);
        let (vertices, indices) = self.shapes.stroke_rounded_rect(
            x,
            y,
            width,
            height,
            line_width,
            corner_radius,
            self.depth,
        );

        draw_color(self, &vertices, &indices, None);
    }

    pub fn image(&mut self, img: &Texture, x: f32, y: f32) {
        self.image_ext(img, x, y, img.width(), img.height(), 0.0, 0.0, 0.0, 0.0);
    }

    pub fn image_crop(
        &mut self,
        img: &Texture,
        x: f32,
        y: f32,
        source_x: f32,
        source_y: f32,
        source_width: f32,
        source_height: f32,
    ) {
        self.image_ext(
            img,
            x,
            y,
            source_width,
            source_height,
            source_x,
            source_y,
            source_width,
            source_height,
        );
    }

    pub fn image_ext(
        &mut self,
        img: &Texture,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        source_x: f32,
        source_y: f32,
        source_width: f32,
        source_height: f32,
    ) {
        if !img.is_loaded() {
            return;
        }

        let frame = img.frame();
        let base_width = img.base_width();
        let base_height = img.base_height();

        let ww = if width == 0.0 {
            frame.width //frame or base_width?
        } else {
            width
        };
        let hh = if height == 0.0 { frame.height } else { height };

        let x2 = x + ww;
        let y2 = y + hh;

        let sx = frame.x + source_x;
        let sy = frame.y + source_y;
        let sw = if source_width == 0.0 {
            frame.width
        } else {
            source_width
        };
        let sh = if source_height == 0.0 {
            frame.height
        } else {
            source_height
        };

        let sx1 = sx / base_width;
        let sy1 = sy / base_height;
        let sx2 = (sx + sw) / base_width;
        let sy2 = (sy + sh) / base_height;

        //http://webglstats.com/webgl/parameter/MAX_TEXTURE_IMAGE_UNITS
        paint_mode(self, PaintMode::Image);

        #[rustfmt::skip]
        draw_image(
            self,
            img,
            &[
                x, y, self.depth,
                x2, y, self.depth,
                x, y2, self.depth,
                x2, y2, self.depth,
            ],
            &[
                sx1, sy1,
                sx2, sy1,
                sx1, sy2,
                sx2, sy2
            ],
            &[
                0, 1, 2, 2, 1, 3
            ]
        );
    }

    pub fn image_9slice(&mut self, img: &Texture, x: f32, y: f32, width: f32, height: f32) {
        let ww = img.width() / 3.0;
        let hh = img.height() / 3.0;
        self.image_9slice_ext(img, x, y, width, height, ww, ww, hh, hh);
    }

    pub fn image_9slice_ext(
        &mut self,
        img: &Texture,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        left: f32,
        right: f32,
        top: f32,
        bottom: f32,
    ) {
        let center_sw = img.width() - (left + right);
        let center_sh = img.height() - (top + bottom);
        let center_w = width - (left + right);
        let center_h = height - (top + bottom);

        self.image_crop(img, x, y, 0.0, 0.0, left, top);
        self.image_ext(img, x + left, y, center_w, top, left, 0.0, center_sw, top);
        self.image_crop(
            img,
            x + left + center_w,
            y,
            left + center_sw,
            0.0,
            right,
            top,
        );

        self.image_ext(img, x, y + top, left, center_h, 0.0, top, left, center_sh);
        self.image_ext(
            img,
            x + left,
            y + top,
            center_w,
            center_h,
            left,
            top,
            center_sw,
            center_sh,
        );
        self.image_ext(
            img,
            x + left + center_w,
            y + top,
            right,
            center_h,
            left + center_sw,
            top,
            right,
            center_sh,
        );

        self.image_crop(
            img,
            x,
            y + top + center_h,
            0.0,
            top + center_sh,
            left,
            bottom,
        );
        self.image_ext(
            img,
            x + left,
            y + top + center_h,
            center_w,
            bottom,
            left,
            top + center_sh,
            center_sw,
            bottom,
        );
        self.image_crop(
            img,
            x + left + center_w,
            y + top + center_h,
            left + center_sw,
            top + center_sh,
            right,
            bottom,
        );
    }

    pub fn pattern(
        &mut self,
        img: &Texture,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        offset_x: f32,
        offset_y: f32,
    ) {
        self.pattern_ext(img, x, y, width, height, offset_x, offset_y, 1.0, 1.0);
    }

    pub fn pattern_ext(
        &mut self,
        img: &Texture,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        offset_x: f32,
        offset_y: f32,
        scale_x: f32,
        scale_y: f32,
    ) {
        paint_mode(self, PaintMode::Pattern);
        if !img.is_loaded() {
            return;
        }

        let frame = img.frame();
        let base_width = img.base_width();
        let base_height = img.base_height();

        let x2 = x + width;
        let y2 = y + height;

        let tex_width = width / frame.width;
        let tex_height = height / frame.height;

        let fract_x = tex_width.fract();
        let fract_y = tex_height.fract();
        let offset_x = (offset_x / frame.width).fract();
        let offset_y = (offset_y / frame.height).fract();
        let sx1 = tex_width.floor() + offset_x;
        let sy1 = tex_height.floor() + offset_y;
        let sx2 = /*(width / tex_width) -*/ offset_x;
        let sy2 = /*(height / tex_height) -*/ offset_y;

        //http://webglstats.com/webgl/parameter/MAX_TEXTURE_IMAGE_UNITS
        paint_mode(self, PaintMode::Pattern);

        #[rustfmt::skip]
            draw_pattern(
            self,
            img,
            &[
                x, y, self.depth,
                x2, y, self.depth,
                x, y2, self.depth,
                x2, y2, self.depth,
            ],
            &[
                sx1, sy1,
                sx2, sy1,
                sx1, sy2,
                sx2, sy2
            ],
            &[
                0, 1, 2, 2, 1, 3
            ]
        );
    }
}

fn flush(draw: &mut Draw) {
    match draw.current_mode {
        PaintMode::Color => draw.color_batcher.flush(
            &mut draw.gfx,
            match &draw.projection {
                Some(p) => p,
                _ => &draw.render_projection,
            },
        ),
        PaintMode::Image => draw.image_batcher.flush(
            &mut draw.gfx,
            match &draw.projection {
                Some(p) => p,
                _ => &draw.render_projection,
            },
        ),
        PaintMode::Pattern => draw.pattern_batcher.flush(
            &mut draw.gfx,
            match &draw.projection {
                Some(p) => p,
                _ => &draw.render_projection,
            },
        ),
        _ => {}
    }
}

fn paint_mode(draw: &mut Draw, mode: PaintMode) {
    if draw.current_mode == mode {
        return;
    }

    flush(draw);
    draw.current_mode = mode;
}

fn draw_color(draw: &mut Draw, vertices: &[f32], indices: &[u32], color: Option<Color>) {
    draw.color_batcher.push_data(
        &mut draw.gfx,
        DrawData {
            vertices,
            indices,
            projection: match &draw.projection {
                Some(p) => p,
                _ => &draw.render_projection,
            },
            matrix: match &draw.matrix {
                Some(p) => p,
                _ => &draw.matrix_stack.last().as_ref().unwrap(),
            },
            blend: draw.blend_mode,
            color: color.unwrap_or(draw.color),
            alpha: draw.alpha,
        },
    );
}

fn draw_image(draw: &mut Draw, texture: &Texture, vertices: &[f32], uvs: &[f32], indices: &[u32]) {
    draw.image_batcher.push_data(
        &mut draw.gfx,
        texture,
        uvs,
        DrawData {
            vertices,
            indices,
            projection: match &draw.projection {
                Some(p) => p,
                _ => &draw.render_projection,
            },
            matrix: match &draw.matrix {
                Some(p) => p,
                _ => &draw.matrix_stack.last().as_ref().unwrap(),
            },
            blend: draw.blend_mode,
            color: draw.color,
            alpha: draw.alpha,
        },
    )
}

fn draw_pattern(
    draw: &mut Draw,
    texture: &Texture,
    vertices: &[f32],
    uvs: &[f32],
    indices: &[u32],
) {
    draw.pattern_batcher.push_data(
        &mut draw.gfx,
        texture,
        uvs,
        DrawData {
            vertices,
            indices,
            projection: match &draw.projection {
                Some(p) => p,
                _ => &draw.render_projection,
            },
            matrix: match &draw.matrix {
                Some(p) => p,
                _ => &draw.matrix_stack.last().as_ref().unwrap(),
            },
            blend: draw.blend_mode,
            color: draw.color,
            alpha: draw.alpha,
        },
    )
}

#[derive(Debug, PartialEq)]
enum PaintMode {
    None,
    Color,
    Image,
    Pattern,
    Text,
    //Particles?
}

pub(crate) struct DrawData<'data> {
    pub vertices: &'data [f32],
    pub indices: &'data [u32],
    pub color: Color,
    pub alpha: f32,
    pub blend: Option<BlendMode>,
    pub projection: &'data Matrix4,
    pub matrix: &'data Matrix4,
}
