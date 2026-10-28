use crate::opbasics::*;

use std::mem;
use std::usize;

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub enum Rotation {
  Normal,
  Rotate90,
  Rotate180,
  Rotate270,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct OpTransform {
  pub rotation: Rotation,
  pub fliph: bool,
  pub flipv: bool,
}

impl OpTransform {
  pub fn new(img: &ImageSource) -> OpTransform {
    match img {
      ImageSource::Raw(img) => {
        let (rotation, fliph, flipv) = match img.orientation {
          Orientation::Normal
          | Orientation::Unknown      => (Rotation::Normal, false, false),
          Orientation::VerticalFlip   => (Rotation::Normal, false, true),
          Orientation::HorizontalFlip => (Rotation::Normal, true, false),
          Orientation::Rotate180      => (Rotation::Rotate180, false, false),
          Orientation::Transpose      => (Rotation::Rotate90, false, true),
          Orientation::Rotate90       => (Rotation::Rotate90, false, false),
          Orientation::Rotate270      => (Rotation::Rotate270, false, false),
          Orientation::Transverse     => (Rotation::Rotate270, true, false),
        };

        OpTransform{
          rotation,
          fliph,
          flipv,
        }
      },
      ImageSource::Other(_) => {
        OpTransform{
          rotation: Rotation::Normal,
          fliph: false,
          flipv: false,
        }
      }
    }
  }
}

impl<'a> ImageOp<'a> for OpTransform {
  fn name(&self) -> &str {"transform"}
  fn run(&self, _pipeline: &PipelineGlobals, buf: Arc<OpBuffer>) -> Arc<OpBuffer> {
    let (f1, f2, f3) = match self.rotation {
      Rotation::Normal    => Orientation::Normal,
      Rotation::Rotate90  => Orientation::Rotate90,
      Rotation::Rotate180 => Orientation::Rotate180,
      Rotation::Rotate270 => Orientation::Rotate270,
    }.to_flips();

    let orientation = Orientation::from_flips((f1, f2 ^ self.fliph, f3 ^ self.flipv));

    if orientation == Orientation::Normal || orientation == Orientation::Unknown {
      buf
    } else {
      Arc::new(rotate_buffer(&buf, &orientation))
    }
  }

  fn transform_forward(&mut self, width: usize, height: usize) -> (usize, usize) {
    match self.rotation {
      Rotation::Rotate90 | Rotation::Rotate270 => (height, width),
      _ => (width, height),
    }
  }

  fn transform_reverse(&mut self, width: usize, height: usize) -> (usize, usize) {
    self.transform_forward(width, height)
  }
}

fn rotate_buffer(buf: &OpBuffer, orientation: &Orientation) -> OpBuffer {
  assert_eq!(buf.colors, 3);

  if *orientation == Orientation::Normal || *orientation == Orientation::Unknown {
    return buf.clone();
  }

  if buf.data.len() >= usize::MAX / 2 {
    panic!("Buffer is too wide or high to rotate");
  }

  let mut width = buf.width as isize;
  let mut height = buf.height as isize;

  let (transpose, flip_x, flip_y) = orientation.to_flips();

  let mut base_offset: isize = 0;
  let mut x_step: isize = 3;
  let mut y_step: isize = width * 3;

  if flip_x {
    x_step = -x_step;
    base_offset += (width - 1) * 3;
  }

  if flip_y {
    y_step = -y_step;
    base_offset += width * (height - 1) * 3;
  }

  let mut out = if transpose {
    mem::swap(&mut width, &mut height);
    mem::swap(&mut x_step, &mut y_step);
    OpBuffer::new(buf.height, buf.width, 3 as usize, buf.monochrome)
  } else {
    OpBuffer::new(buf.width, buf.height, 3 as usize, buf.monochrome)
  };

  out.mutate_lines(&(|line: &mut [f32], row| {
    let line_offset = base_offset + y_step * row as isize;
    for col in 0..width {
      let offset = line_offset + x_step * col;
      for c in 0..3 {
        line[(col * 3 + c) as usize] = buf.data[(offset + c) as usize];
      }
    }
  }));

  out
}
