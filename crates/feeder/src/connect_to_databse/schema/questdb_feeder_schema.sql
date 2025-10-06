-- QuestDB Schema for Feeder Operational Logs
-- This schema tracks feeder health, performance, and errors for analysis

-- 1. Connection Events Log
-- Tracks WebSocket connection lifecycle events
CREATE TABLE IF NOT EXISTS feeder_connection_logs (
    timestamp TIMESTAMP,
    exchange SYMBOL,
    connection_id SYMBOL,
    event_type SYMBOL,  -- connected, disconnected, reconnecting, rate_limited, error
    status SYMBOL,      -- success, failure, pending
    error_msg STRING,
    symbols_count INT,
    reconnect_attempt INT,
    retry_after_ms LONG
) timestamp(timestamp) PARTITION BY DAY;

-- 2. Performance Metrics Log
-- System performance metrics collected every 10 seconds
CREATE TABLE IF NOT EXISTS feeder_performance_logs (
    timestamp TIMESTAMP,
    exchange SYMBOL,
    cpu_usage DOUBLE,        -- CPU usage percentage
    memory_mb DOUBLE,        -- Memory usage in MB
    websocket_count INT,     -- Active WebSocket connections
    data_rate DOUBLE,        -- Messages per second
    latency_ms DOUBLE,       -- Average processing latency
    dropped_messages LONG,   -- Total dropped messages
    queue_depth INT          -- Current message queue depth
) timestamp(timestamp) PARTITION BY DAY;

-- 3. Error Logs
-- Detailed error tracking for debugging
CREATE TABLE IF NOT EXISTS feeder_error_logs (
    timestamp TIMESTAMP,
    level SYMBOL,           -- ERROR, WARN, INFO, DEBUG
    module SYMBOL,          -- Component that generated the error
    exchange SYMBOL,
    error_code STRING,
    message STRING,
    context STRING,         -- JSON context data
    stack_trace STRING
) timestamp(timestamp) PARTITION BY DAY;

-- 4. Processing Statistics
-- Data processing statistics per exchange and data type
CREATE TABLE IF NOT EXISTS feeder_stats_logs (
    timestamp TIMESTAMP,
    exchange SYMBOL,
    data_type SYMBOL,       -- trade, orderbook, ticker
    messages_received LONG,
    messages_processed LONG,
    dropped_count LONG,
    parsing_errors INT,
    avg_processing_time_us DOUBLE
) timestamp(timestamp) PARTITION BY DAY;

-- 5. Aggregated Hourly Stats (Materialized View equivalent)
-- For faster queries on historical data
CREATE TABLE IF NOT EXISTS feeder_hourly_stats (
    hour TIMESTAMP,
    exchange SYMBOL,
    total_messages LONG,
    total_errors INT,
    avg_cpu_usage DOUBLE,
    avg_memory_mb DOUBLE,
    max_latency_ms DOUBLE,
    connection_drops INT,
    uptime_percent DOUBLE
) timestamp(hour) PARTITION BY MONTH;

-- Indexes for common queries (QuestDB creates these automatically on SYMBOL columns)
-- No explicit index creation needed in QuestDB

-- Sample queries for monitoring:

-- Get recent connection events
-- SELECT * FROM feeder_connection_logs 
-- WHERE timestamp > dateadd('h', -1, now()) 
-- ORDER BY timestamp DESC;

-- Check current performance
-- SELECT exchange, avg(cpu_usage), avg(memory_mb), avg(latency_ms) 
-- FROM feeder_performance_logs 
-- WHERE timestamp > dateadd('m', -5, now()) 
-- GROUP BY exchange;

-- Count errors by exchange in last hour
-- SELECT exchange, count() as error_count 
-- FROM feeder_error_logs 
-- WHERE timestamp > dateadd('h', -1, now()) 
-- GROUP BY exchange;

-- Message processing rate
-- SELECT exchange, data_type, 
--        sum(messages_processed) / 60 as messages_per_second
-- FROM feeder_stats_logs
-- WHERE timestamp > dateadd('m', -1, now())
-- GROUP BY exchange, data_type;