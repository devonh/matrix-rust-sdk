use std::{collections::HashMap, sync::Arc};

use matrix_sdk_crypto::dehydrated_machine::{
    DehydratedDevice as InnerDehydratedDevice, DehydratedDevices as InnerDehydratedDevices,
    RehydratedDevice as InnerRehydratedDevice,
};
use ruma::{api::client::dehydrated_device, events::AnyToDeviceEvent, serde::Raw, OwnedDeviceId};
use tokio::runtime::Handle;
use zeroize::Zeroize;

#[derive(uniffi::Object)]
pub struct DehydratedDevices {
    runtime: Handle,
    inner: InnerDehydratedDevices,
}

#[uniffi::export]
impl DehydratedDevices {
    pub fn create(&self) -> Arc<DehydratedDevice> {
        DehydratedDevice { inner: self.inner.create(), runtime: self.runtime.to_owned() }.into()
    }

    pub fn rehydrate(
        &self,
        pickle_key: Vec<u8>,
        device_id: String,
        device_data: String,
    ) -> Arc<RehydratedDevice> {
        let device_data: Raw<_> = serde_json::from_str(&device_data).unwrap();
        let device_id: OwnedDeviceId = device_id.into();
        let mut key: [u8; 32] = pickle_key.try_into().unwrap();

        let ret = RehydratedDevice {
            runtime: self.runtime.to_owned(),
            inner: self
                .runtime
                .block_on(self.inner.rehydrate(&key, &device_id, device_data))
                .unwrap(),
        }
        .into();

        key.zeroize();

        ret
    }
}

#[derive(uniffi::Object)]
pub struct RehydratedDevice {
    inner: InnerRehydratedDevice,
    runtime: Handle,
}

#[uniffi::export]
impl RehydratedDevice {
    pub fn receive_events(&self, events: String) {
        let events: Vec<Raw<AnyToDeviceEvent>> = serde_json::from_str(&events).unwrap();
        self.runtime.block_on(self.inner.receive_events(events)).unwrap();
    }
}

#[derive(uniffi::Object)]
pub struct DehydratedDevice {
    runtime: Handle,
    inner: InnerDehydratedDevice,
}

impl DehydratedDevice {
    pub fn keys_for_upload(
        &self,
        device_display_name: String,
        pickle_key: Vec<u8>,
    ) -> UploadDehydratedDeviceRequest {
        let mut key: [u8; 32] = pickle_key.try_into().unwrap();

        let request = self.runtime.block_on(self.inner.keys_for_upload(device_display_name, &key));

        key.zeroize();

        request.into()
    }
}

#[derive(uniffi::Record)]
pub struct UploadDehydratedDeviceRequest {
    /// The unique ID of the device.
    device_id: String,

    /// The display name of the device.
    initial_device_display_name: String,

    /// The data of the dehydrated device, containing the serialized and
    /// encrypted private parts of the [`DeviceKeys`].
    device_data: String,

    /// Identity keys for the device.
    ///
    /// May be absent if no new identity keys are required.
    pub device_keys: String,

    /// One-time public keys for "pre-key" messages.
    pub one_time_keys: HashMap<String, String>,

    /// Fallback public keys for "pre-key" messages.
    pub fallback_keys: HashMap<String, String>,
}

impl From<dehydrated_device::put_dehydrated_device::unstable::Request>
    for UploadDehydratedDeviceRequest
{
    fn from(value: dehydrated_device::put_dehydrated_device::unstable::Request) -> Self {
        Self {
            device_id: value.device_id.to_string(),
            initial_device_display_name: value.initial_device_display_name,
            device_data: value.device_data.json().get().to_owned(),
            device_keys: value.device_keys.json().get().to_owned(),
            one_time_keys: value
                .one_time_keys
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.json().get().to_owned()))
                .collect(),
            fallback_keys: value
                .fallback_keys
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.json().get().to_owned()))
                .collect(),
        }
    }
}
