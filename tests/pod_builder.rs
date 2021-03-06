use k8s_openapi::api::core::v1::{Container, Pod, Volume, VolumeMount};
use serde_json::json;
use std::sync::Arc;

pub struct PodLifetimeOwner {
    pub pod: Pod,
    _tempdirs: Vec<Arc<tempfile::TempDir>>, // only to keep the directories alive
}

pub struct WasmerciserContainerSpec {
    pub name: &'static str,
    pub args: &'static [&'static str],
}

pub struct WasmerciserVolumeSpec {
    pub volume_name: &'static str,
    pub mount_path: &'static str,
    pub source: WasmerciserVolumeSource,
}

pub enum WasmerciserVolumeSource {
    HostPath,
    ConfigMap(&'static str),
    ConfigMapItems(&'static str, Vec<(&'static str, &'static str)>),
    Secret(&'static str),
    SecretItems(&'static str, Vec<(&'static str, &'static str)>),
}

fn wasmerciser_container(
    spec: &WasmerciserContainerSpec,
    volumes: &Vec<WasmerciserVolumeSpec>,
) -> anyhow::Result<Container> {
    let volume_mounts: Vec<_> = volumes
        .iter()
        .map(|v| wasmerciser_volume_mount(v).unwrap())
        .collect();
    let container: Container = serde_json::from_value(json!({
        "name": spec.name,
        "image": "webassembly.azurecr.io/wasmerciser:v0.2.0",
        "args": spec.args,
        "volumeMounts": volume_mounts,
    }))?;
    Ok(container)
}

fn wasmerciser_volume_mount(spec: &WasmerciserVolumeSpec) -> anyhow::Result<VolumeMount> {
    let mount: VolumeMount = serde_json::from_value(json!({
        "mountPath": spec.mount_path,
        "name": spec.volume_name
    }))?;
    Ok(mount)
}

fn wasmerciser_volume(
    spec: &WasmerciserVolumeSpec,
) -> anyhow::Result<(Volume, Option<Arc<tempfile::TempDir>>)> {
    match spec.source {
        WasmerciserVolumeSource::HostPath => {
            let tempdir = Arc::new(tempfile::tempdir()?);

            let volume: Volume = serde_json::from_value(json!({
                "name": spec.volume_name,
                "hostPath": {
                    "path": tempdir.path()
                }
            }))?;

            Ok((volume, Some(tempdir)))
        }
        WasmerciserVolumeSource::ConfigMap(name) => {
            let volume: Volume = serde_json::from_value(json!({
                "name": spec.volume_name,
                "configMap": {
                    "name": name,
                }
            }))?;

            Ok((volume, None))
        }
        WasmerciserVolumeSource::ConfigMapItems(name, ref items) => {
            let volume: Volume = serde_json::from_value(json!({
                "name": spec.volume_name,
                "configMap": {
                    "name": name,
                    "items": items.iter().map(|(key, path)| json!({"key": key, "path": path})).collect::<Vec<_>>(),
                }
            }))?;

            Ok((volume, None))
        }
        WasmerciserVolumeSource::Secret(name) => {
            let volume: Volume = serde_json::from_value(json!({
                "name": spec.volume_name,
                "secret": {
                    "secretName": name,
                }
            }))?;

            Ok((volume, None))
        }
        WasmerciserVolumeSource::SecretItems(name, ref items) => {
            let volume: Volume = serde_json::from_value(json!({
                "name": spec.volume_name,
                "secret": {
                    "secretName": name,
                    "items": items.iter().map(|(key, path)| json!({"key": key, "path": path})).collect::<Vec<_>>(),
                }
            }))?;

            Ok((volume, None))
        }
    }
}

pub fn wasmerciser_pod(
    pod_name: &str,
    inits: Vec<WasmerciserContainerSpec>,
    containers: Vec<WasmerciserContainerSpec>,
    test_volumes: Vec<WasmerciserVolumeSpec>,
    architecture: &str,
) -> anyhow::Result<PodLifetimeOwner> {
    let init_container_specs: Vec<_> = inits
        .iter()
        .map(|spec| wasmerciser_container(spec, &test_volumes).unwrap())
        .collect();
    let app_container_specs: Vec<_> = containers
        .iter()
        .map(|spec| wasmerciser_container(spec, &test_volumes).unwrap())
        .collect();

    let volume_maps: Vec<_> = test_volumes
        .iter()
        .map(|spec| wasmerciser_volume(spec).unwrap())
        .collect();
    let (volumes, tempdirs) = unzip(&volume_maps);

    let pod = serde_json::from_value(json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": pod_name
        },
        "spec": {
            "initContainers": init_container_specs,
            "containers": app_container_specs,
            "tolerations": [
                {
                    "effect": "NoExecute",
                    "key": "kubernetes.io/arch",
                    "operator": "Equal",
                    "value": architecture,
                },
            ],
            "nodeSelector": {
                "kubernetes.io/arch": architecture
            },
            "volumes": volumes,
        }
    }))?;

    Ok(PodLifetimeOwner {
        pod,
        _tempdirs: option_values(&tempdirs),
    })
}

fn unzip<T, U: Clone>(source: &Vec<(T, U)>) -> (Vec<&T>, Vec<U>) {
    let ts: Vec<_> = source.iter().map(|v| &v.0).collect();
    let us: Vec<_> = source.iter().map(|v| v.1.clone()).collect();
    (ts, us)
}

fn option_values<T: Clone>(source: &Vec<Option<T>>) -> Vec<T> {
    source.iter().filter_map(|t| t.clone()).collect()
}
