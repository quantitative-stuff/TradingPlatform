// This module aggregates all Binance OMS functionality.
// It includes both REST- and WebSocket-based implementations for sending orders
// and for checking order status messages.

pub mod binance_order;             // REST-based order sender (BinanceOrderClient)
pub mod binance_ws_order;          // WebSocket-based order sender (BinanceWsOrderClient)
pub mod binance_order_status;      // REST-based order status checker (BinanceRestOrderStatusChecker)
pub mod binance_ws_order_status;   // WebSocket-based order status checker (BinanceWsOrderStatusChecker)
pub mod binance_risk_managers;    // Risk managers for Binance

pub use binance_order::BinanceOrderClient;
pub use binance_ws_order::BinanceWsOrderClient;
pub use binance_order_status::BinanceRestOrderStatusChecker;
pub use binance_ws_order_status::BinanceWsOrderStatusChecker;
pub use binance_risk_managers::BinanceRiskManager;  