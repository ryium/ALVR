//! Driver declarations are not real-time decoding benchmarks.
use alvr_common::frameconnect::{
    CAPABILITY_VERSION, DecodeMode, FrameConnectCapabilities, HardwareDecoder,
};
use jni::{
    errors::Result,
    jni_sig, jni_str,
    objects::{JObject, JObjectArray, JString},
};

pub fn probe(vm: &jni::JavaVM) -> FrameConnectCapabilities {
    let result = vm.attach_current_thread(|env| {
        let list = env.new_object(
            jni_str!("android/media/MediaCodecList"),
            jni_sig!("(I)V"),
            &[0.into()],
        )?;
        let infos = env
            .call_method(
                &list,
                jni_str!("getCodecInfos"),
                jni_sig!("()[Landroid/media/MediaCodecInfo;"),
                &[],
            )?
            .l()?;
        let infos = env.cast_local::<JObjectArray<JObject>>(infos)?;
        let mut decoders = vec![];
        for index in 0..infos.len(env)? {
            let info = infos.get_element(env, index)?;
            if env
                .call_method(&info, jni_str!("isEncoder"), jni_sig!("()Z"), &[])?
                .z()?
            {
                continue;
            }
            // API 29+. Report failure instead of guessing from a codec name on older platforms.
            if !env
                .call_method(
                    &info,
                    jni_str!("isHardwareAccelerated"),
                    jni_sig!("()Z"),
                    &[],
                )?
                .z()?
            {
                continue;
            }
            let name = env
                .call_method(
                    &info,
                    jni_str!("getName"),
                    jni_sig!("()Ljava/lang/String;"),
                    &[],
                )?
                .l()?;
            let name = env.cast_local::<JString>(name)?.to_string();
            let types = env
                .call_method(
                    &info,
                    jni_str!("getSupportedTypes"),
                    jni_sig!("()[Ljava/lang/String;"),
                    &[],
                )?
                .l()?;
            let types = env.cast_local::<JObjectArray<JString>>(types)?;
            for type_index in 0..types.len(env)? {
                let mime = types.get_element(env, type_index)?;
                let mime_string = mime.to_string();
                if !matches!(mime_string.as_str(), "video/av01" | "video/hevc") {
                    continue;
                }
                let caps = env
                    .call_method(
                        &info,
                        jni_str!("getCapabilitiesForType"),
                        jni_sig!(
                            "(Ljava/lang/String;)Landroid/media/MediaCodecInfo$CodecCapabilities;"
                        ),
                        &[(&mime).into()],
                    )?
                    .l()?;
                let video = env
                    .call_method(
                        &caps,
                        jni_str!("getVideoCapabilities"),
                        jni_sig!("()Landroid/media/MediaCodecInfo$VideoCapabilities;"),
                        &[],
                    )?
                    .l()?;
                let profiles = env
                    .get_field(
                        &caps,
                        jni_str!("profileLevels"),
                        jni_sig!("[Landroid/media/MediaCodecInfo$CodecProfileLevel;"),
                    )?
                    .l()?;
                let profiles = env.cast_local::<JObjectArray<JObject>>(profiles)?;
                let mut ten_bit = false;
                for p in 0..profiles.len(env)? {
                    let profile = profiles.get_element(env, p)?;
                    let value = env
                        .get_field(&profile, jni_str!("profile"), jni_sig!("I"))?
                        .i()?;
                    ten_bit |= matches!(value, 2 | 4096 | 8192);
                }
                let mut modes = vec![];
                for eye_size in [2176_u32, 2560, 2880] {
                    for fps in [90.0_f32, 120.0, 144.0] {
                        if env
                            .call_method(
                                &video,
                                jni_str!("areSizeAndRateSupported"),
                                jni_sig!("(IID)Z"),
                                &[
                                    ((eye_size * 2) as i32).into(),
                                    (eye_size as i32).into(),
                                    (fps as f64).into(),
                                ],
                            )?
                            .z()?
                        {
                            modes.push(DecodeMode {
                                width: eye_size * 2,
                                height: eye_size,
                                fps,
                            });
                        }
                    }
                }
                decoders.push(HardwareDecoder {
                    name: name.clone(),
                    mime: mime_string,
                    ten_bit,
                    modes,
                    rate_source: "mediacodec-size-and-rate".into(),
                });
            }
        }
        Result::Ok(decoders)
    });
    match result {
        Ok(decoders) => FrameConnectCapabilities {
            version: CAPABILITY_VERSION,
            decoders,
            ..Default::default()
        },
        Err(e) => {
            let _ = vm.attach_current_thread(|env| {
                env.exception_clear();
                Result::Ok(())
            });
            FrameConnectCapabilities {
                version: CAPABILITY_VERSION,
                probe_error: Some(e.to_string()),
                ..Default::default()
            }
        }
    }
}
