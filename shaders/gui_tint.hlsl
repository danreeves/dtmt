// Example Darktide pixel shader.
//
// This replaces the `gui` shader's diffuse-map permutation used by
// `content/ui/materials/base/ui_default_base`. It keeps the shipped shader's
// interface exactly:
//
//   inputs   SV_Position, CUSTOM0 (color), CUSTOM1 (uv), CUSTOM2
//   output   SV_Target0
//   t0       global_texture2D[]  (bindless texture descriptors, space2)
//   s0       global_samplers[]   (bindless sampler descriptors, space2)
//   b0       global_viewport     (engine viewport constants)
//   b1       c_per_object        (per-object constants, including the material's
//                                 bindless descriptor indices)
//
// The texture and sampler are not bound directly: the material writes
// descriptor indices into `c_per_object`, exactly like the shipped shader.
//
// Compile with `shaders/compile.ps1` (see `shaders/README.md`), then splice the
// container into a material with the `shader43` SDK example.

Texture2D<float4> global_texture2D[] : register(t0, space2);
SamplerState global_samplers[] : register(s0, space2);

struct GlobalViewport {
    float3 camera_unprojection;
    float3 camera_pos;
    float4x4 camera_view;
    float4x4 camera_inv_view;
    float4x4 camera_world;
    float4x4 camera_last_world;
    float4x4 camera_last_view;
    float4x4 camera_last_inv_view;
    float4x4 camera_projection[2];
    float4x4 camera_inv_projection[2];
    float4x4 camera_last_projection[2];
    float4x4 camera_last_inv_projection[2];
    float4x4 camera_last_view_projection[2];
    float4x4 camera_last_inv_view_projection[2];
    float4x4 camera_view_projection[2];
    float4x4 camera_inv_view_projection[2];
    float time;
    float delta_time;
    float2 streamer_write_feedback_threshold;
    float2 sampler_lod_bias;
    float frame_number;
    float2 back_buffer_size;
    float2 output_rt_size;
    float4x4 hdr_content_to_monitor_rec;
    float taa_enabled;
    float jitter_enabled;
    float upscaling_enabled;
    float debug_rendering;
    float gamma;
    float lens_quality_color_fringe_enabled;
    float lens_quality_distortion_enabled;
    float volumetric_reprojection_amount;
    float volumetric_volumes_enabled;
    float sun_shadows;
    float capture_cubemap;
    float capture_ddgi;
    float hair_wrapped_diffuse;
    float4 viewport;
    float is_editor;
    float2 mouse_uv;
    float rt_reflections_enabled;
    float rt_particle_reflections_enabled;
    float rt_relax_denoiser_enabled;
    float dlss_rr_enabled;
    float gtao_enabled;
    float cacao_enabled;
    float rt_mixed_reflections;
    float rt_shadow_ray_multiplier;
    float rtxgi_enabled;
    float baked_ddgi;
    float dxr;
    float rt_checkerboard_reflections;
    float3 camera_near_far;
    float direct_diffuse_enabled;
    float direct_specular_enabled;
    float indirect_diffuse_enabled;
    float indirect_specular_enabled;
    float occlusion_debug_opacity;
    float occlusion_debug_far;
    float occlusion_debug_near;
    float occluder_debug_opacity;
    float occluder_debug_far;
    float hdr_paper_white_nits;
    float debug_hdr_compare;
    float reverse_z;
    float terrain_displacement_min_distance;
    float terrain_displacement_max_distance;
    float terrain_tesselation_min_distance;
    float terrain_tesselation_max_distance;
};
ConstantBuffer<GlobalViewport> global_viewport : register(b0);

struct PerObject {
    uint bindless_sampler_texture_map;
    uint2 bindless_tex2d_texture_map;
    uint2 bindless_minlod_texture_map;
    float4x4 view_proj;
    float4x4 world_view_proj;
    float4x4 world;
    float4 dev_wireframe_color;
};
ConstantBuffer<PerObject> c_per_object : register(b1);

struct PS_INPUT {
    float4 position : SV_Position;
    float4 color : CUSTOM0;
    float2 uv : CUSTOM1;
    float3 extra : CUSTOM2;
};

// Tint applied to the sampled texture. The pulse uses the engine's shader clock
// so the change is obviously live.
static const float3 TINT = float3(1.0, 0.3, 0.3);

float4 ps_main(PS_INPUT input) : SV_Target0 {
    uint sampler_index = c_per_object.bindless_sampler_texture_map;
    uint texture_index = c_per_object.bindless_tex2d_texture_map.x;

    SamplerState samp = global_samplers[NonUniformResourceIndex(sampler_index)];
    Texture2D<float4> tex = global_texture2D[NonUniformResourceIndex(texture_index)];

    float4 c = tex.Sample(samp, input.uv);
    c.rgb *= TINT;

    float pulse = 0.5 + 0.5 * cos(global_viewport.time * 4.0);
    c.rgb *= 0.75 + 0.25 * pulse;

    float alpha = c.a * input.color.a;
    return float4(c.rgb * input.color.rgb * alpha, alpha);
}
