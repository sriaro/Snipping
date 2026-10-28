use crate::opbasics::*;
use std::f32::consts::FRAC_PI_2;

static EPSILON: f32 = 1.0 / 1000000.0;

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct OpRotateCrop {
  pub crop_top: f32,
  pub crop_right: f32,
  pub crop_bottom: f32,
  pub crop_left: f32,
  pub rotation: f32,
  input_ratio: f32,
  output_size: Option<(usize, usize)>,
}

impl OpRotateCrop {
  pub fn new(_img: &ImageSource) -> Self {
    Self::empty()
  }
  pub fn empty() -> Self {
    Self {
      crop_top: 0.0,
      crop_right: 0.0,
      crop_bottom: 0.0,
      crop_left: 0.0,
      rotation: 0.0,
      input_ratio: 1.0,
      output_size: None,
    }
  }
}

impl<'a> ImageOp<'a> for OpRotateCrop {
  fn name(&self) -> &str {"rotatecrop"}
  fn run(&self, _pipeline: &PipelineGlobals, buf: Arc<OpBuffer>) -> Arc<OpBuffer> {
    if self.noop() { return buf; }

    let (swidth, sheight) = (buf.width as f32, buf.height as f32);
    let (nwidth, nheight) = self.calc_size(buf.width, buf.height, false);
    let (fnwidth, fnheight) = (nwidth as f32, nheight as f32);

    let x = (swidth * self.crop_left).floor();
    if x < 0.0 || x > swidth {
      log::error!("Trying to crop left outside image");
      return buf;
    }
    let y = (sheight * self.crop_top).floor();
    if y < 0.0 || y > sheight {
      log::error!("Trying to crop top outside image");
      return buf;
    }

    let topleft = self.rotate_point_reverse(x, y, fnwidth, fnheight, swidth, sheight);
    let topright = self.rotate_point_reverse(x + fnwidth - 1.0, y, fnwidth, fnheight, swidth, sheight);
    let bottomleft = self.rotate_point_reverse(x, y + fnheight - 1.0, fnwidth, fnheight, swidth, sheight);
    let newbuffer = buf.transform(topleft, topright, bottomleft, nwidth, nheight);
    Arc::new(newbuffer)
  }

  fn transform_forward(&mut self, width: usize, height: usize) -> (usize, usize) {
    if let Some(size) = self.output_size {
      size
    } else {
      self.input_ratio = width as f32 / height as f32;
      self.calc_size(width, height, false)
    }
  }

  fn transform_reverse(&mut self, width: usize, height: usize) -> (usize, usize) {
    self.output_size = Some((width, height));
    self.calc_size(width, height, true)
  }

  fn reset(&mut self) {
    self.input_ratio = 1.0;
    self.output_size = None;
  }
}

impl OpRotateCrop {
  fn noop(&self) -> bool {
    self.rotation.abs() < EPSILON &&
    self.crop_top.abs() < EPSILON &&
    self.crop_right.abs() < EPSILON &&
    self.crop_bottom.abs() < EPSILON &&
    self.crop_left.abs() < EPSILON
  }

  fn rotate_point_reverse(&self, x: f32, y: f32, width: f32, height: f32, swidth: f32, sheight: f32) -> (isize, isize) {
    if self.rotation < EPSILON {
      (x as isize, y as isize)
    } else {
      let angle = FRAC_PI_2 * if self.rotation > 1.0 {1.0} else {self.rotation};
      let (sin, cos) = angle.sin_cos();
      let (tx, ty) = (x - (width / 2.0), y - (height / 2.0));
      let nx = tx*cos + ty*sin + (swidth / 2.0);
      let ny = - tx*sin + ty*cos + (sheight / 2.0);
      (nx as isize, ny as isize)
    }
  }

  fn calc_size(&self, owidth: usize, oheight: usize, reverse: bool) -> (usize, usize){
    if self.noop() { return (owidth, oheight); }

    let (width, height) = (owidth as f32, oheight as f32);

    let (width, height) = if reverse || self.rotation < EPSILON {
      (width, height)
    } else {
      let angle = FRAC_PI_2 * if self.rotation > 1.0 {1.0} else {self.rotation};
      let (sin, cos) = angle.sin_cos();
      (width*cos + height*sin, width*sin + height*cos)
    };

    let nwidth = {
      let ratio = 1.0 - self.crop_left - self.crop_right;
      let nwidth = if reverse {
        (width / ratio).round()
      } else {
        (width * ratio).round()
      };
      if ratio < EPSILON || nwidth < 1.0 {
        log::error!("Trying to crop width beyond limits");
        return (owidth, oheight);
      }
      nwidth
    };

    let nheight = {
      let ratio = 1.0 - self.crop_top - self.crop_bottom;
      let nheight = if reverse {
        (height / ratio).round()
      } else {
        (height * ratio).round()
      };
      if ratio < EPSILON || nheight < 1.0 {
        log::error!("Trying to crop height beyond limits");
        return (owidth, oheight);
      }
      nheight
    };

    let (nwidth, nheight) = if !reverse || self.rotation < EPSILON {
      (nwidth, nheight)
    } else {
      let angle = FRAC_PI_2 * if self.rotation > 1.0 {1.0} else {self.rotation};
      let (sin, cos) = angle.sin_cos();
      let width = (nheight / (sin + (cos/self.input_ratio))).round();
      let height = (width / self.input_ratio).round();
      (width, height)
    };

    (nwidth as usize, nheight as usize)
  }
}
