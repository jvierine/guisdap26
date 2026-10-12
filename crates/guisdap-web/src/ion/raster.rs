use super::super::{el, error, shader};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext as Gl};
pub fn draw(
    id: &str,
    values: &[f32],
    width: usize,
    height: usize,
    seismic: bool,
) -> Result<(), JsValue> {
    let canvas: HtmlCanvasElement = el(id).dyn_into()?;
    let gl: Gl = canvas
        .get_context("webgl2")?
        .ok_or_else(|| error("WebGL2 required"))?
        .dyn_into()?;
    let vert=shader(&gl,Gl::VERTEX_SHADER,"#version 300 es\nprecision highp float;out vec2 uv;void main(){vec2 p=vec2(float((gl_VertexID<<1)&2),float(gl_VertexID&2));uv=p;gl_Position=vec4(p*2.-1.,0,1);}")?;
    let frag = shader(
        &gl,
        Gl::FRAGMENT_SHADER,
        r#"#version 300 es
precision highp float;in vec2 uv;out vec4 color;uniform sampler2D field;uniform bool seismic;
const vec3 palette[8]=vec3[8](vec3(13,8,135),vec3(84,3,160),vec3(139,10,165),vec3(185,50,137),vec3(219,92,104),vec3(244,136,73),vec3(254,188,43),vec3(240,249,33));
void main(){float z=texture(field,uv).r;if(isnan(z)){color=vec4(.04,.07,.10,1);return;}bool flagged=z>=2.;float p=clamp(flagged?z-2.:z,0.,1.)*7.;int i=min(int(floor(p)),6);vec3 c=mix(palette[i],palette[i+1],p-float(i))/255.;if(seismic){vec3 a[5]=vec3[5](vec3(0,0,.3),vec3(0,0,1),vec3(1),vec3(1,0,0),vec3(.5,0,0));float q=clamp(flagged?z-2.:z,0.,1.)*4.;int j=min(int(floor(q)),3);c=mix(a[j],a[j+1],q-float(j));}if(flagged&&mod(gl_FragCoord.x+gl_FragCoord.y,16.)<1.)c=mix(c,vec3(.8),.22);color=vec4(c,1);}
"#,
    )?;
    let program = gl
        .create_program()
        .ok_or_else(|| error("program allocation"))?;
    gl.attach_shader(&program, &vert);
    gl.attach_shader(&program, &frag);
    gl.link_program(&program);
    if gl
        .get_program_parameter(&program, Gl::LINK_STATUS)
        .as_bool()
        != Some(true)
    {
        return Err(error(gl.get_program_info_log(&program).unwrap_or_default()));
    }
    gl.use_program(Some(&program));
    gl.uniform1i(
        gl.get_uniform_location(&program, "seismic").as_ref(),
        i32::from(seismic),
    );
    let texture = gl
        .create_texture()
        .ok_or_else(|| error("texture allocation"))?;
    gl.bind_texture(Gl::TEXTURE_2D, Some(&texture));
    for (key, value) in [
        (Gl::TEXTURE_MIN_FILTER, Gl::NEAREST),
        (Gl::TEXTURE_MAG_FILTER, Gl::NEAREST),
        (Gl::TEXTURE_WRAP_S, Gl::CLAMP_TO_EDGE),
        (Gl::TEXTURE_WRAP_T, Gl::CLAMP_TO_EDGE),
    ] {
        gl.tex_parameteri(Gl::TEXTURE_2D, key, value as i32);
    }
    let data = js_sys::Float32Array::from(values);
    gl.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_array_buffer_view(
        Gl::TEXTURE_2D,
        0,
        Gl::R32F as i32,
        width as i32,
        height as i32,
        0,
        Gl::RED,
        Gl::FLOAT,
        Some(&data),
    )?;
    gl.viewport(0, 0, canvas.width() as i32, canvas.height() as i32);
    gl.draw_arrays(Gl::TRIANGLES, 0, 3);
    gl.delete_texture(Some(&texture));
    gl.delete_program(Some(&program));
    gl.delete_shader(Some(&vert));
    gl.delete_shader(Some(&frag));
    if gl.get_error() != Gl::NO_ERROR {
        return Err(error("ion texture rendering failed"));
    }
    Ok(())
}
