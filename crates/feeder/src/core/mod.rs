mod market_data;
mod types;
mod order_types;
mod traits;
mod order_traits;
mod order_manager;
mod connection_manager;
mod logging;
pub mod connection_stats;
pub mod udp_sender;
pub mod optimized_udp_sender;
pub mod ordered_udp_sender;
pub mod packet_validator;
pub mod asset_precision;
pub mod enhanced_packet;
pub mod file_logger;
pub mod terminal_monitor;
pub mod network_utils;
pub mod feeder_metrics;
pub mod safe_logging;
pub mod robust_connection;
pub mod timestamp_normalizer;
pub mod timestamp_filter;
pub mod buffered_udp_sender;
pub mod binary_udp_packet;
pub mod binary_udp_sender;
pub mod market_cache;
pub mod symbol_mapper_cache;
pub mod multi_port_udp_sender;
pub mod multi_port_udp_receiver;
pub mod fast_json;

// Export feeder metrics functions
pub use feeder_metrics::{
    init_feeder_logger,
    log_connection,
    log_performance,
    log_error,
    log_processing_stats,
};

pub use market_data::{
    get_market_price,
    MarketDataProvider,
};

pub use order_manager::{
    OrderManager,
    TrackedOrder,
    ORDER_MANAGER,
};

// Explicitly import types
pub use order_types::{
    OrderSide,
    OrderType,
    OrderRequest,
    OrderResponse,
    Position,
    Balance,
};

// Explicitly import traits
pub use order_traits::{
    OrderExecutor,
    OrderStatusChecker,
    RiskManager,
    OrderNotificationHandler,
    Strategy,

};

// Import from types module if needed
pub use types::{
    MarketData, 
    MarketDataType,
    OrderBookData, 
    TradeData, 
    SymbolMapper,
    TRADES, 
    ORDERBOOKS, 
    COMPARE_NOTIFY,
    get_shutdown_receiver,
    trigger_shutdown
};


pub use traits::{
    TradeComparator, 
    Feeder, 
    Storage
};



pub use connection_manager::{
    ConnectionManager,
    ConnectionConfig,
    ConnectionStatus,
    ConnectionSummary,
};

pub use logging::{
    LoggingConfig,
    setup_logging,
    setup_console_logging,
};

pub use file_logger::{
    FileLogger,
    suppress_console_output,
};

pub use terminal_monitor::TerminalMonitor;

pub use connection_stats::{ConnectionStats, CONNECTION_STATS};
pub use udp_sender::{UdpSender, init_global_udp_sender, get_udp_sender};
pub use optimized_udp_sender::{
    OptimizedUdpSender, 
    init_global_optimized_udp_sender, 
    get_optimized_udp_sender,
    UdpSenderCompat
};
pub use ordered_udp_sender::{
    OrderedUdpSender,
    init_global_ordered_udp_sender,
    get_ordered_udp_sender
};
pub use packet_validator::{
    PacketValidator,
    init_packet_validator,
    get_packet_validator,
    validate_before_send,
    validate_after_receive
};
pub use asset_precision::{
    AssetPrecision,
    PrecisionManager,
    PRECISION_MANAGER,
    scale_price,
    unscale_price,
    get_price_decimals,
    update_precision_from_exchange
};

pub use safe_logging::{
    safe_json_log,
    safe_error_log, 
    safe_message_log,
    safe_json_parse_error_log,
    safe_connection_error_log,
    safe_websocket_message_log,
    truncate_for_log,
};

pub use robust_connection::{
    ExchangeConnectionLimits,
    RobustConnectionManager,
};

pub use timestamp_normalizer::{
    normalize_timestamp,
    detect_format,
    TimestampFormat,
};

pub use timestamp_filter::{
    is_valid_timestamp,
    process_timestamp_field,
    get_current_timestamp_ms,
    fix_binance_timestamp,
};

pub use buffered_udp_sender::BufferedUdpSender;
pub use binary_udp_packet::{
    PacketHeader,
    OrderBookItem,
    TradeItem,
    BinaryUdpPacket
};
pub use binary_udp_sender::{
    BinaryUdpSender,
    init_global_binary_udp_sender,
    get_binary_udp_sender,
    BinaryUdpSenderCompat
};
pub use market_cache::{
    SymbolSlot,
    MarketSnapshot,
    MarketCache,
    SharedMarketCache,
    CacheStats,
};
pub use symbol_mapper_cache::{
    SymbolMapper as CacheSymbolMapper,
    map_exchange,
    map_symbol,
    get_exchange_name,
    get_symbol_name,
};
pub use multi_port_udp_sender::{
    MultiPortUdpSender,
    init_global_multi_port_sender,
    get_multi_port_sender,
};
pub use fast_json::{
    FastJsonParser,
    BinanceTrade,
    BinanceOrderBook,
    get_f64,
    get_i64,
    get_u64,
    get_str,
    get_bool,
    get_array,
};
pub mod spawning; 
