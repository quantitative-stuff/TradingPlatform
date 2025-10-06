"""
Enhanced LS Securities API System with Futures and Options Support
================================================================

This enhanced system provides:
1. Stock master database
2. Futures and options master database  
3. Real-time data for all asset types
4. Historical data for derivatives
5. Comprehensive market data coverage

Asset Types Supported:
- 주식 (Stocks)
- 선물 (Futures) - KOSPI200, KQ150, etc.
- 옵션 (Options) - Call/Put options
- 해외선물 (Overseas Futures)
"""

import requests
import json
import sqlite3
import asyncio
import websockets
import pandas as pd
from datetime import datetime, timedelta
import logging
from typing import List, Dict, Optional
from dataclasses import dataclass
from enum import Enum

# Configure logging
logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')
logger = logging.getLogger(__name__)

class AssetType(Enum):
    """Asset type enumeration"""
    STOCK = "stock"
    FUTURES = "futures" 
    OPTIONS = "options"
    OVERSEAS_FUTURES = "overseas_futures"
    ETF = "etf"
    ELW = "elw"

class MarketCode(Enum):
    """Market code enumeration"""
    KOSPI = "1"
    KOSDAQ = "2"
    KOTC = "3"
    FUTURES = "4"
    OPTIONS = "5"
    
@dataclass
class DerivativeInfo:
    """Derivative instrument information"""
    symbol: str
    name: str
    underlying: str
    asset_type: AssetType
    expiry_date: str
    strike_price: float = None
    option_type: str = None  # 'C' for Call, 'P' for Put
    multiplier: int = 1

class LSSecuritiesEnhancedAPI:
    def __init__(self, app_key, app_secret, is_production=False):
        """
        Initialize Enhanced LS Securities API client with derivatives support
        """
        self.app_key = app_key
        self.app_secret = app_secret
        self.access_token = None
        
        if is_production:
            self.base_url = "https://openapi.ls-sec.co.kr:8080"
            self.websocket_url = "wss://openapi.ls-sec.co.kr:9443/websocket"
        else:
            self.base_url = "https://openapi.ls-sec.co.kr:8080"
            self.websocket_url = "wss://openapi.ls-sec.co.kr:9443/websocket"
        
        self.db_path = "comprehensive_market_data.db"
        self.init_enhanced_database()
        
    def init_enhanced_database(self):
        """Initialize comprehensive SQLite database for all asset types"""
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        
        # Stock master table
        cursor.execute('''
            CREATE TABLE IF NOT EXISTS stock_master (
                stock_code TEXT PRIMARY KEY,
                stock_name TEXT,
                market_code TEXT,
                sector_code TEXT,
                industry_name TEXT,
                listing_date TEXT,
                capital INTEGER,
                shares_outstanding INTEGER,
                face_value INTEGER,
                asset_type TEXT DEFAULT 'stock',
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )
        ''')
        
        # Futures master table
        cursor.execute('''
            CREATE TABLE IF NOT EXISTS futures_master (
                futures_code TEXT PRIMARY KEY,
                futures_name TEXT,
                underlying_asset TEXT,
                contract_size INTEGER,
                tick_size REAL,
                tick_value REAL,
                expiry_date TEXT,
                delivery_month TEXT,
                margin_rate REAL,
                last_trading_day TEXT,
                settlement_method TEXT,
                asset_type TEXT DEFAULT 'futures',
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )
        ''')
        
        # Options master table  
        cursor.execute('''
            CREATE TABLE IF NOT EXISTS options_master (
                option_code TEXT PRIMARY KEY,
                option_name TEXT,
                underlying_asset TEXT,
                option_type TEXT,  -- 'C' for Call, 'P' for Put
                strike_price REAL,
                expiry_date TEXT,
                contract_size INTEGER,
                tick_size REAL,
                tick_value REAL,
                last_trading_day TEXT,
                settlement_method TEXT,
                asset_type TEXT DEFAULT 'options',
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )
        ''')
        
        # Overseas futures master table
        cursor.execute('''
            CREATE TABLE IF NOT EXISTS overseas_futures_master (
                symbol TEXT PRIMARY KEY,
                name TEXT,
                exchange TEXT,
                currency TEXT,
                contract_size REAL,
                tick_size REAL,
                margin_rate REAL,
                trading_hours TEXT,
                asset_type TEXT DEFAULT 'overseas_futures',
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )
        ''')
        
        # Universal real-time data table for all asset types
        cursor.execute('''
            CREATE TABLE IF NOT EXISTS realtime_data (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                symbol TEXT,
                asset_type TEXT,
                current_price REAL,
                change_price REAL,
                change_rate REAL,
                volume INTEGER,
                trading_value REAL,
                high_price REAL,
                low_price REAL,
                open_price REAL,
                bid_price REAL,
                ask_price REAL,
                bid_volume INTEGER,
                ask_volume INTEGER,
                open_interest INTEGER,  -- For futures/options
                implied_volatility REAL,  -- For options
                delta REAL,  -- For options
                gamma REAL,  -- For options
                theta REAL,  -- For options
                vega REAL,   -- For options
                timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )
        ''')
        
        # ETF master table
        cursor.execute('''
            CREATE TABLE IF NOT EXISTS etf_master (
                etf_code TEXT PRIMARY KEY,
                etf_name TEXT,
                benchmark TEXT,
                nav REAL,
                tracking_error REAL,
                expense_ratio REAL,
                fund_size REAL,
                inception_date TEXT,
                asset_type TEXT DEFAULT 'etf',
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )
        ''')
        
        # Create indexes for better performance
        cursor.execute('CREATE INDEX IF NOT EXISTS idx_realtime_symbol ON realtime_data(symbol)')
        cursor.execute('CREATE INDEX IF NOT EXISTS idx_realtime_asset_type ON realtime_data(asset_type)')
        cursor.execute('CREATE INDEX IF NOT EXISTS idx_realtime_timestamp ON realtime_data(timestamp)')
        cursor.execute('CREATE INDEX IF NOT EXISTS idx_futures_expiry ON futures_master(expiry_date)')
        cursor.execute('CREATE INDEX IF NOT EXISTS idx_options_expiry ON options_master(expiry_date)')
        cursor.execute('CREATE INDEX IF NOT EXISTS idx_options_strike ON options_master(strike_price)')
        
        conn.commit()
        conn.close()
        logger.info("Enhanced database initialized successfully")
    
    def get_access_token(self):
        """Get OAuth access token"""
        url = f"{self.base_url}/oauth2/token"
        headers = {
            "content-type": "application/x-www-form-urlencoded"
        }
        data = {
            "grant_type": "client_credentials",
            "appkey": self.app_key,
            "appsecretkey": self.app_secret
        }
        
        try:
            response = requests.post(url, headers=headers, data=data)
            response.raise_for_status()
            token_data = response.json()
            self.access_token = token_data.get('access_token')
            logger.info("Access token obtained successfully")
            return self.access_token
        except Exception as e:
            logger.error(f"Failed to get access token: {e}")
            return None
    
    # Stock-related methods (existing)
    def get_stock_master_list(self):
        """Get stock master list"""
        if not self.access_token:
            self.get_access_token()
        
        url = f"{self.base_url}/stock/market-data"
        headers = {
            "content-type": "application/json; charset=utf-8",
            "authorization": f"Bearer {self.access_token}",
            "tr_cd": "t8436",
            "tr_cont": "N",
            "tr_cont_key": ""
        }
        
        body = {"t8436InBlock": {"gubun": "0"}}
        
        try:
            response = requests.post(url, headers=headers, data=json.dumps(body))
            response.raise_for_status()
            data = response.json()
            self._save_stock_master_data(data)
            logger.info("Stock master data updated successfully")
        except Exception as e:
            logger.error(f"Failed to get stock master list: {e}")
    
    # Futures-related methods
    def get_futures_master_list(self):
        """Get futures master list (선물 종목정보 - t2301)"""
        if not self.access_token:
            self.get_access_token()
        
        url = f"{self.base_url}/fuopt/market-data"
        headers = {
            "content-type": "application/json; charset=utf-8",
            "authorization": f"Bearer {self.access_token}",
            "tr_cd": "t2301",  # 선물종목정보조회
            "tr_cont": "N",
            "tr_cont_key": ""
        }
        
        body = {"t2301InBlock": {"dummy": ""}}
        
        try:
            response = requests.post(url, headers=headers, data=json.dumps(body))
            response.raise_for_status()
            data = response.json()
            self._save_futures_master_data(data)
            logger.info("Futures master data updated successfully")
        except Exception as e:
            logger.error(f"Failed to get futures master list: {e}")
    
    def get_futures_current_price(self, futures_code):
        """Get current futures price (선물현재가 - t2101)"""
        if not self.access_token:
            self.get_access_token()
        
        url = f"{self.base_url}/fuopt/market-data"
        headers = {
            "content-type": "application/json; charset=utf-8",
            "authorization": f"Bearer {self.access_token}",
            "tr_cd": "t2101",
            "tr_cont": "N",
            "tr_cont_key": ""
        }
        
        body = {"t2101InBlock": {"focode": futures_code}}
        
        try:
            response = requests.post(url, headers=headers, data=json.dumps(body))
            response.raise_for_status()
            return response.json()
        except Exception as e:
            logger.error(f"Failed to get futures current price for {futures_code}: {e}")
            return None
    
    # Options-related methods
    def get_options_master_list(self):
        """Get options master list (옵션 종목정보 - t2301)"""
        if not self.access_token:
            self.get_access_token()
        
        url = f"{self.base_url}/fuopt/market-data"
        headers = {
            "content-type": "application/json; charset=utf-8",
            "authorization": f"Bearer {self.access_token}",
            "tr_cd": "t2301",  # 옵션종목정보조회
            "tr_cont": "N",
            "tr_cont_key": ""
        }
        
        # Get both call and put options
        for option_type in ["2", "3"]:  # 2: Call, 3: Put
            body = {"t2301InBlock": {"gubun": option_type}}
            
            try:
                response = requests.post(url, headers=headers, data=json.dumps(body))
                response.raise_for_status()
                data = response.json()
                self._save_options_master_data(data, option_type)
                logger.info(f"Options master data updated for type {option_type}")
            except Exception as e:
                logger.error(f"Failed to get options master list for type {option_type}: {e}")
    
    def get_options_current_price(self, option_code):
        """Get current options price (옵션현재가 - t2105)"""
        if not self.access_token:
            self.get_access_token()
        
        url = f"{self.base_url}/fuopt/market-data"
        headers = {
            "content-type": "application/json; charset=utf-8",
            "authorization": f"Bearer {self.access_token}",
            "tr_cd": "t2105",
            "tr_cont": "N",
            "tr_cont_key": ""
        }
        
        body = {"t2105InBlock": {"focode": option_code}}
        
        try:
            response = requests.post(url, headers=headers, data=json.dumps(body))
            response.raise_for_status()
            return response.json()
        except Exception as e:
            logger.error(f"Failed to get options current price for {option_code}: {e}")
            return None
    
    # Overseas futures methods
    def get_overseas_futures_master_list(self):
        """Get overseas futures master list"""
        if not self.access_token:
            self.get_access_token()
        
        url = f"{self.base_url}/overseas-fuopt/market-data"
        headers = {
            "content-type": "application/json; charset=utf-8",
            "authorization": f"Bearer {self.access_token}",
            "tr_cd": "o3101",  # 해외선물 종목정보
            "tr_cont": "N",
            "tr_cont_key": ""
        }
        
        body = {"o3101InBlock": {"dummy": ""}}
        
        try:
            response = requests.post(url, headers=headers, data=json.dumps(body))
            response.raise_for_status()
            data = response.json()
            self._save_overseas_futures_master_data(data)
            logger.info("Overseas futures master data updated successfully")
        except Exception as e:
            logger.error(f"Failed to get overseas futures master list: {e}")
    
    def get_overseas_futures_current_price(self, symbol):
        """Get current overseas futures price"""
        if not self.access_token:
            self.get_access_token()
        
        url = f"{self.base_url}/overseas-fuopt/market-data"
        headers = {
            "content-type": "application/json; charset=utf-8",
            "authorization": f"Bearer {self.access_token}",
            "tr_cd": "o3103",
            "tr_cont": "N",
            "tr_cont_key": ""
        }
        
        body = {"o3103InBlock": {"symbol": symbol}}
        
        try:
            response = requests.post(url, headers=headers, data=json.dumps(body))
            response.raise_for_status()
            return response.json()
        except Exception as e:
            logger.error(f"Failed to get overseas futures current price for {symbol}: {e}")
            return None
    
    # ETF methods
    def get_etf_master_list(self):
        """Get ETF master list"""
        if not self.access_token:
            self.get_access_token()
        
        url = f"{self.base_url}/stock/market-data"
        headers = {
            "content-type": "application/json; charset=utf-8",
            "authorization": f"Bearer {self.access_token}",
            "tr_cd": "t1404",  # ETF 정보조회
            "tr_cont": "N",
            "tr_cont_key": ""
        }
        
        body = {"t1404InBlock": {"gubun": "E"}}  # E for ETF
        
        try:
            response = requests.post(url, headers=headers, data=json.dumps(body))
            response.raise_for_status()
            data = response.json()
            self._save_etf_master_data(data)
            logger.info("ETF master data updated successfully")
        except Exception as e:
            logger.error(f"Failed to get ETF master list: {e}")
    
    # Data saving methods
    def _save_stock_master_data(self, data):
        """Save stock master data to database"""
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        
        if 't8436OutBlock' in data:
            for item in data['t8436OutBlock']:
                cursor.execute('''
                    INSERT OR REPLACE INTO stock_master 
                    (stock_code, stock_name, market_code, sector_code, industry_name, 
                     listing_date, capital, shares_outstanding, face_value, asset_type, updated_at)
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'stock', CURRENT_TIMESTAMP)
                ''', (
                    item.get('shcode', ''),
                    item.get('hname', ''),
                    item.get('gubun', ''),
                    item.get('etfgubun', ''),
                    item.get('industry', ''),
                    item.get('listdate', ''),
                    item.get('capital', 0),
                    item.get('listing_cnt', 0),
                    item.get('par', 0)
                ))
        
        conn.commit()
        conn.close()
    
    def _save_futures_master_data(self, data):
        """Save futures master data to database"""
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        
        if 't2301OutBlock' in data:
            for item in data['t2301OutBlock']:
                cursor.execute('''
                    INSERT OR REPLACE INTO futures_master 
                    (futures_code, futures_name, underlying_asset, contract_size, tick_size, 
                     tick_value, expiry_date, delivery_month, margin_rate, last_trading_day, 
                     settlement_method, asset_type, updated_at)
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'futures', CURRENT_TIMESTAMP)
                ''', (
                    item.get('focode', ''),
                    item.get('hname', ''),
                    item.get('basecode', ''),
                    item.get('contunit', 0),
                    item.get('ticksize', 0.0),
                    item.get('tickvalue', 0.0),
                    item.get('lastdate', ''),
                    item.get('month', ''),
                    item.get('margin', 0.0),
                    item.get('lasttradedate', ''),
                    item.get('settlement', '')
                ))
        
        conn.commit()
        conn.close()
    
    def _save_options_master_data(self, data, option_type):
        """Save options master data to database"""
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        
        option_type_map = {"2": "C", "3": "P"}  # Call/Put mapping
        
        if 't2301OutBlock' in data:
            for item in data['t2301OutBlock']:
                cursor.execute('''
                    INSERT OR REPLACE INTO options_master 
                    (option_code, option_name, underlying_asset, option_type, strike_price,
                     expiry_date, contract_size, tick_size, tick_value, last_trading_day,
                     settlement_method, asset_type, updated_at)
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'options', CURRENT_TIMESTAMP)
                ''', (
                    item.get('focode', ''),
                    item.get('hname', ''),
                    item.get('basecode', ''),
                    option_type_map.get(option_type, 'C'),
                    float(item.get('actprice', 0)),
                    item.get('lastdate', ''),
                    item.get('contunit', 0),
                    item.get('ticksize', 0.0),
                    item.get('tickvalue', 0.0),
                    item.get('lasttradedate', ''),
                    item.get('settlement', '')
                ))
        
        conn.commit()
        conn.close()
    
    def _save_overseas_futures_master_data(self, data):
        """Save overseas futures master data to database"""
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        
        if 'o3101OutBlock' in data:
            for item in data['o3101OutBlock']:
                cursor.execute('''
                    INSERT OR REPLACE INTO overseas_futures_master 
                    (symbol, name, exchange, currency, contract_size, tick_size, 
                     margin_rate, trading_hours, asset_type, updated_at)
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'overseas_futures', CURRENT_TIMESTAMP)
                ''', (
                    item.get('symbol', ''),
                    item.get('symname', ''),
                    item.get('exchange', ''),
                    item.get('currency', ''),
                    float(item.get('contsize', 0)),
                    float(item.get('ticksize', 0)),
                    float(item.get('margin', 0)),
                    item.get('tradinghours', '')
                ))
        
        conn.commit()
        conn.close()
    
    def _save_etf_master_data(self, data):
        """Save ETF master data to database"""
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        
        if 't1404OutBlock' in data:
            for item in data['t1404OutBlock']:
                cursor.execute('''
                    INSERT OR REPLACE INTO etf_master 
                    (etf_code, etf_name, benchmark, nav, tracking_error, expense_ratio,
                     fund_size, inception_date, asset_type, updated_at)
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'etf', CURRENT_TIMESTAMP)
                ''', (
                    item.get('shcode', ''),
                    item.get('hname', ''),
                    item.get('benchmark', ''),
                    float(item.get('nav', 0)),
                    float(item.get('trackingerror', 0)),
                    float(item.get('expenseratio', 0)),
                    float(item.get('fundsize', 0)),
                    item.get('inceptiondate', '')
                ))
        
        conn.commit()
        conn.close()
    
    # Enhanced WebSocket support for all asset types
    async def connect_enhanced_websocket(self, symbols_by_type: Dict[str, List[str]]):
        """Connect to WebSocket for real-time data across all asset types"""
        if not self.access_token:
            self.get_access_token()
        
        try:
            async with websockets.connect(self.websocket_url) as websocket:
                logger.info("Enhanced WebSocket connected successfully")
                
                # Subscribe to different asset types
                subscription_map = {
                    'stocks': 'S3_',        # Real-time stock price
                    'futures': 'F1_',       # Real-time futures price
                    'options': 'O1_',       # Real-time options price
                    'overseas_futures': 'OF_', # Real-time overseas futures
                    'etf': 'S3_'           # ETFs use same as stocks
                }
                
                for asset_type, symbols in symbols_by_type.items():
                    tr_cd = subscription_map.get(asset_type, 'S3_')
                    
                    for symbol in symbols:
                        subscribe_message = {
                            "header": {
                                "token": self.access_token,
                                "tr_type": "3"  # 실시간 등록
                            },
                            "body": {
                                "tr_cd": tr_cd,
                                "tr_key": symbol
                            }
                        }
                        
                        await websocket.send(json.dumps(subscribe_message))
                        logger.info(f"Subscribed to {asset_type} real-time data for {symbol}")
                
                # Listen for real-time data
                async for message in websocket:
                    try:
                        data = json.loads(message)
                        await self._process_enhanced_realtime_data(data)
                    except json.JSONDecodeError:
                        logger.error(f"Failed to parse message: {message}")
                    except Exception as e:
                        logger.error(f"Error processing real-time data: {e}")
        
        except Exception as e:
            logger.error(f"Enhanced WebSocket connection failed: {e}")
    
    async def _process_enhanced_realtime_data(self, data):
        """Process and store enhanced real-time data for all asset types"""
        try:
            if 'body' in data and 'header' in data:
                body = data['body']
                tr_cd = data.get('header', {}).get('tr_cd', '')
                
                # Determine asset type from TR code
                asset_type_map = {
                    'S3_': 'stock',
                    'F1_': 'futures', 
                    'O1_': 'options',
                    'OF_': 'overseas_futures'
                }
                
                asset_type = asset_type_map.get(tr_cd, 'unknown')
                
                conn = sqlite3.connect(self.db_path)
                cursor = conn.cursor()
                
                # Enhanced data insertion with derivatives-specific fields
                cursor.execute('''
                    INSERT INTO realtime_data 
                    (symbol, asset_type, current_price, change_price, change_rate, 
                     volume, trading_value, high_price, low_price, open_price,
                     bid_price, ask_price, bid_volume, ask_volume, open_interest,
                     implied_volatility, delta, gamma, theta, vega)
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ''', (
                    body.get('shcode', body.get('symbol', '')),
                    asset_type,
                    float(body.get('price', 0)),
                    float(body.get('change', 0)),
                    float(body.get('chrate', 0.0)),
                    int(body.get('volume', 0)),
                    float(body.get('value', 0)),
                    float(body.get('high', 0)),
                    float(body.get('low', 0)),
                    float(body.get('open', 0)),
                    float(body.get('offerho1', 0)),  # Bid price
                    float(body.get('bidho1', 0)),    # Ask price
                    int(body.get('offerrem1', 0)),   # Bid volume
                    int(body.get('bidrem1', 0)),     # Ask volume
                    int(body.get('openinterest', 0)), # Open interest (futures/options)
                    float(body.get('impvol', 0.0)),  # Implied volatility (options)
                    float(body.get('delta', 0.0)),   # Delta (options)
                    float(body.get('gamma', 0.0)),   # Gamma (options)  
                    float(body.get('theta', 0.0)),   # Theta (options)
                    float(body.get('vega', 0.0))     # Vega (options)
                ))
                
                conn.commit()
                conn.close()
                
                logger.info(f"Enhanced real-time data saved for {asset_type}: {body.get('shcode', 'Unknown')}")
                
        except Exception as e:
            logger.error(f"Failed to process enhanced real-time data: {e}")
    
    # Query methods for all asset types
    def get_all_assets_summary(self) -> Dict[str, int]:
        """Get summary of all assets in database"""
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        
        summary = {}
        
        # Count each asset type
        tables = [
            ('stocks', 'stock_master'),
            ('futures', 'futures_master'), 
            ('options', 'options_master'),
            ('overseas_futures', 'overseas_futures_master'),
            ('etfs', 'etf_master')
        ]
        
        for asset_type, table_name in tables:
            cursor.execute(f"SELECT COUNT(*) FROM {table_name}")
            count = cursor.fetchone()[0]
            summary[asset_type] = count
        
        # Real-time data counts by asset type
        cursor.execute("SELECT asset_type, COUNT(*) FROM realtime_data GROUP BY asset_type")
        realtime_counts = cursor.fetchall()
        summary['realtime_data'] = {asset_type: count for asset_type, count in realtime_counts}
        
        conn.close()
        return summary
    
    def get_derivatives_by_underlying(self, underlying_symbol: str) -> Dict[str, List]:
        """Get all derivatives (futures/options) for an underlying asset"""
        conn = sqlite3.connect(self.db_path)
        
        # Get futures
        futures_query = """
            SELECT * FROM futures_master 
            WHERE underlying_asset LIKE ? 
            ORDER BY expiry_date
        """
        futures_df = pd.read_sql_query(futures_query, conn, params=[f"%{underlying_symbol}%"])
        
        # Get options
        options_query = """
            SELECT * FROM options_master 
            WHERE underlying_asset LIKE ? 
            ORDER BY expiry_date, strike_price
        """
        options_df = pd.read_sql_query(options_query, conn, params=[f"%{underlying_symbol}%"])
        
        conn.close()
        
        return {
            'futures': futures_df.to_dict('records'),
            'options': options_df.to_dict('records')
        }
    
    def get_options_chain(self, underlying: str, expiry_date: str) -> Dict[str, List]:
        """Get options chain for specific underlying and expiry"""
        conn = sqlite3.connect(self.db_path)
        
        query = """
            SELECT * FROM options_master 
            WHERE underlying_asset LIKE ? AND expiry_date = ?
            ORDER BY strike_price, option_type
        """
        
        options_df = pd.read_sql_query(query, conn, params=[f"%{underlying}%", expiry_date])
        conn.close()
        
        calls = options_df[options_df['option_type'] == 'C'].to_dict('records')
        puts = options_df[options_df['option_type'] == 'P'].to_dict('records')
        
        return {'calls': calls, 'puts': puts}

# Example usage for enhanced system
def main_enhanced():
    """Example usage of enhanced system"""
    # Initialize enhanced API
    api = LSSecuritiesEnhancedAPI(
        app_key="YOUR_APP_KEY",
        app_secret="YOUR_APP_SECRET",
        is_production=False
    )
    
    print("🚀 Enhanced LS Securities API System")
    print("=" * 50)
    
    # Update all master databases
    print("📥 Updating master databases...")
    api.get_stock_master_list()
    api.get_futures_master_list()
    api.get_options_master_list()
    api.get_overseas_futures_master_list()
    api.get_etf_master_list()
    
    # Get summary
    summary = api.get_all_assets_summary()
    print(f"\n📊 Asset Summary:")
    for asset_type, count in summary.items():
        if asset_type != 'realtime_data':
            print(f"   {asset_type.title()}: {count}")
    
    # Get KOSPI200 derivatives
    print(f"\n📈 KOSPI200 Derivatives:")
    kospi_derivatives = api.get_derivatives_by_underlying("KOSPI200")
    print(f"   Futures: {len(kospi_derivatives['futures'])}")
    print(f"   Options: {len(kospi_derivatives['options'])}")
    
    # Example real-time monitoring for all asset types
    print(f"\n🔴 Starting enhanced real-time monitoring...")
    symbols_by_type = {
        'stocks': ['005930', '000660'],      # Samsung, SK Hynix
        'futures': ['101RC000', '106RC000'], # KOSPI200 futures
        'options': ['201RC240', '301RC240'], # KOSPI200 options
        'etf': ['069500', '114800']          # KODEX200, KODEX인버스
    }
    
    # asyncio.run(api.connect_enhanced_websocket(symbols_by_type))

if __name__ == "__main__":
    main_enhanced()
