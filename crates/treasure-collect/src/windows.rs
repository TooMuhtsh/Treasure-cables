//! Collecte sous Windows.
//!
//! À implémenter : parcours des hubs façon USBView (IOCTL_USB_GET_NODE_*,
//! IOCTL_USB_GET_PORT_CONNECTOR_PROPERTIES, BOS via
//! IOCTL_USB_GET_DESCRIPTOR_FROM_NODE_CONNECTION). Voir
//! docs/recherche-sources-windows.md.

use treasure_model::Report;

use crate::{Error, empty_report};

pub fn collect() -> Result<Report, Error> {
    let mut report = empty_report(None);
    report
        .notes
        .push("Collecte Windows pas encore implémentée.".to_owned());
    Ok(report)
}
