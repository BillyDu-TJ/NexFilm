#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include "libraw/libraw.h"

extern "C" {

struct NexFilmRawMosaicInfo {
  uint32_t raw_width;
  uint32_t raw_height;
  uint32_t active_left;
  uint32_t active_top;
  uint32_t active_width;
  uint32_t active_height;
  uint32_t raw_pitch;
  uint32_t filters;
  uint32_t cfa_kind;
  uint32_t orientation;
  float black_level[4];
  float white_level[4];
  int32_t masked_areas[8][4];
  uint32_t masked_area_count;
  float iso;
  float exposure_seconds;
  char camera_id[160];
  char libraw_version[64];
};

int nexfilm_raw_mosaic_info(libraw_data_t *data, NexFilmRawMosaicInfo *out) {
  if (!data || !out || !data->rawdata.raw_image) {
    return -1;
  }
  std::memset(out, 0, sizeof(*out));
  const libraw_image_sizes_t &sizes = data->rawdata.sizes;
  const libraw_iparams_t &identity = data->rawdata.iparams;
  const libraw_colordata_t &color = data->rawdata.color;
  out->raw_width = sizes.raw_width;
  out->raw_height = sizes.raw_height;
  out->active_left = sizes.left_margin;
  out->active_top = sizes.top_margin;
  out->active_width = sizes.width;
  out->active_height = sizes.height;
  out->raw_pitch = sizes.raw_pitch;
  out->filters = identity.filters;
  out->cfa_kind = identity.filters == LIBRAW_XTRANS ? 2u : (identity.filters ? 1u : 0u);
  out->orientation = static_cast<uint32_t>(std::max(sizes.flip, 0));
  for (int channel = 0; channel < 4; ++channel) {
    out->black_level[channel] = static_cast<float>(color.black + color.cblack[channel]);
    const unsigned linear_maximum = color.linear_max[channel];
    out->white_level[channel] = static_cast<float>(linear_maximum ? linear_maximum : color.maximum);
  }
  for (int area = 0; area < 8; ++area) {
    bool nonempty = false;
    for (int coordinate = 0; coordinate < 4; ++coordinate) {
      out->masked_areas[out->masked_area_count][coordinate] = sizes.mask[area][coordinate];
      nonempty = nonempty || sizes.mask[area][coordinate] != 0;
    }
    if (nonempty) {
      ++out->masked_area_count;
    }
  }
  out->iso = data->other.iso_speed;
  out->exposure_seconds = data->other.shutter;
  const char *make = identity.normalized_make[0] ? identity.normalized_make : identity.make;
  const char *model = identity.normalized_model[0] ? identity.normalized_model : identity.model;
  std::snprintf(out->camera_id, sizeof(out->camera_id), "%s|%s", make, model);
  const char *version = libraw_version();
  if (version) {
    std::snprintf(out->libraw_version, sizeof(out->libraw_version), "%s", version);
  }
  return 0;
}

struct NexFilmRawWhiteBalance {
  float cam_mul[4];
  float pre_mul[4];
  int32_t camera_wb_valid;
};

// Reads the multipliers LibRaw resolved while opening the file: cam_mul is the
// camera's as-shot white balance (AsShotNeutral), pre_mul the fixed daylight
// balance LibRaw falls back to when as-shot WB is switched off.
int nexfilm_raw_white_balance(libraw_data_t *data, NexFilmRawWhiteBalance *out) {
  if (!data || !out) {
    return -1;
  }
  std::memset(out, 0, sizeof(*out));
  const libraw_colordata_t &color = data->rawdata.color;
  for (int channel = 0; channel < 4; ++channel) {
    out->cam_mul[channel] = color.cam_mul[channel];
    out->pre_mul[channel] = color.pre_mul[channel];
  }
  out->camera_wb_valid = color.cam_mul[0] > 0.0f && color.cam_mul[1] > 0.0f &&
                                 color.cam_mul[2] > 0.0f
                             ? 1
                             : 0;
  return 0;
}

// Writes explicit per-channel multipliers and disables both of LibRaw's own
// white-balance choices, so the caller decides the channel balance exactly.
int nexfilm_raw_set_user_mul(libraw_data_t *data, const float *mul, int32_t count) {
  if (!data || !mul || count < 3) {
    return -1;
  }
  for (int32_t channel = 0; channel < count && channel < 4; ++channel) {
    data->params.user_mul[channel] = mul[channel];
  }
  data->params.use_camera_wb = 0;
  data->params.use_auto_wb = 0;
  return 0;
}

int nexfilm_copy_raw_mosaic(libraw_data_t *data, uint16_t *out, size_t capacity) {
  if (!data || !out || !data->rawdata.raw_image) {
    return -1;
  }
  const size_t count = static_cast<size_t>(data->rawdata.sizes.raw_width) *
                       static_cast<size_t>(data->rawdata.sizes.raw_height);
  if (capacity < count) {
    return -2;
  }
  const size_t source_stride = data->rawdata.sizes.raw_pitch / sizeof(uint16_t);
  const size_t width = data->rawdata.sizes.raw_width;
  const size_t height = data->rawdata.sizes.raw_height;
  if (source_stride < width) {
    return -3;
  }
  for (size_t row = 0; row < height; ++row) {
    std::copy_n(data->rawdata.raw_image + row * source_stride,
                width,
                out + row * width);
  }
  return 0;
}

}
