use serde::{Deserialize, Serialize};
use crate::{Side, Timestamp, Price, Quantity};

/// Level 3 Order Event (reconstructed from L2 order book updates)
///
/// These events represent inferred individual order actions derived from
/// aggregated L2 order book changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderEvent {
    Add {
        order_id: u64,
        side: Side,
        price: Price,
        quantity: Quantity,
        timestamp: Timestamp,
    },
    Modify {
        order_id: u64,
        new_quantity: Quantity,
        timestamp: Timestamp,
    },
    Cancel {
        order_id: u64,
        timestamp: Timestamp,
    },
    Execute {
        order_id: u64,
        executed_quantity: Quantity,
        timestamp: Timestamp,
    },
}
