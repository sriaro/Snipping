use crate::opbasics::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpDemosaic {
  pub cfa: String,
}

impl OpDemosaic {
  pub fn new(img: &ImageSource) -> OpDemosaic {
    match img {
      ImageSource::Raw(img) => {
        OpDemosaic{
          cfa: img.cropped_cfa().to_string(),
        }
      },
      ImageSource::Other(_) => {
        OpDemosaic{
          cfa: "".to_string(),
        }
      }
    }
  }
}

impl<'a> ImageOp<'a> for OpDemosaic {
  fn name(&self) -> &str {"demosaic"}
  fn run(&self, pipeline: &PipelineGlobals, buf: Arc<OpBuffer>) -> Arc<OpBuffer> {
    let nwidth = pipeline.settings.demosaic_width;
    let nheight = pipeline.settings.demosaic_height;
    let scale = crate::scaling::calculate_scale(buf.width, buf.height, nwidth, nheight);

    let cfa = CFA::new(&self.cfa);
    let minscale = match cfa.width {
      2  => 2.0,
      6  => 3.0,
      8  => 2.0,
      12 => 12.0,
      _  => 2.0,
    };

    if scale <= 1.0 && buf.colors == 4 {
      buf
    } else if buf.colors == 4 {
      Arc::new(crate::scaling::scale_down_opbuf(&buf, nwidth, nheight))
    } else if scale >= minscale {
      Arc::new(crate::scaling::scaled_demosaic(cfa, &buf, nwidth, nheight))
    } else {
      let fullsize = full(cfa, &buf);
      if scale > 1.0 {
        Arc::new(crate::scaling::scale_down_opbuf(&fullsize, nwidth, nheight))
      } else {
        Arc::new(fullsize)
      }
    }
  }

}

pub fn full(cfa: CFA, buf: &OpBuffer) -> OpBuffer {
  let mut out = OpBuffer::new(buf.width, buf.height, 4, buf.monochrome);

  let offsets3x3: [(isize,isize);9] = [
    (-1,-1), (-1, 0), (-1, 1),
    ( 0,-1), ( 0, 0), ( 0, 1),
    ( 1,-1), ( 1, 0), ( 1, 1),
  ];

  let mut lookups = [[[0;9];48];48];
  for (row, line) in lookups.iter_mut().enumerate() {
    for (col, colors) in line.iter_mut().enumerate() {
      let pixcolor = cfa.color_at(row, col);

      for (i, o) in offsets3x3.iter().enumerate() {
        let (dy, dx) = *o;
        let row = (48+dy) as usize + row;
        let col = (48+dx) as usize + col;
        let ocolor = cfa.color_at(row, col);
        colors[i] = if ocolor != pixcolor || (dx == 0 && dy == 0) { ocolor } else { 4 };
      }
    }
  }

  out.mutate_lines(&(|line: &mut [f32], row| {
    for (col, pix) in line.chunks_exact_mut(4).enumerate() {
      let ref colors = lookups[row%48][col%48];
      let mut sums = [0f32;5];
      let mut counts = [0f32;5];

      for (i, o) in offsets3x3.iter().enumerate() {
        let (dy, dx) = *o;
        let row = row as isize + dy;
        let col = col as isize + dx;
        if row >= 0 && row < (buf.height as isize) &&
           col >= 0 && col < (buf.width as isize) {
          sums[colors[i]] += buf.data[(row as usize)*buf.width+(col as usize)];
          counts[colors[i]] += 1.0;
        }
      }

      for c in 0..4 {
        if counts[c] > 0.0 {
          pix[c] = sums[c] / counts[c];
        }
      }
    }
  }));

  out
}
