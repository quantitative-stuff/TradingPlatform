"""
Enhanced Transaction Codes and Utilities for LS Securities Derivatives
====================================================================

Complete mapping of LS Securities API transaction codes for:
- Futures (선물)
- Options (옵션) 
- Overseas Futures (해외선물)
- ETF/ELW
- Enhanced utilities for derivatives analysis
"""

from dataclasses import dataclass
from typing import Dict, List, Optional
from enum import Enum
from datetime import datetime, timedelta
import pandas as pd
import json

class LSEnhancedTransactionCodes:
    """Complete LS Securities API Transaction Codes for all asset types"""
    
    # === STOCK MARKET DATA ===
    STOCK_CURRENT_PRICE = "t1101"          # 주식현재가호가
    STOCK_TIME_SERIES = "t1301"            # 주식시간대별체결
    STOCK_DAILY_CHART = "t1302"            # 주식일별차트
    STOCK_MINUTE_CHART = "t8412"           # 주식분별차트
    STOCK_INFO = "t1404"                   # 주식기본정보
    STOCK_MASTER_LIST = "t8436"            # 종목정보
    
    # === FUTURES MARKET DATA ===
    FUTURES_MASTER_LIST = "t2301"          # 선물종목정보조회
    FUTURES_CURRENT_PRICE = "t2101"        # 선물현재가
    FUTURES_CHART_DATA = "t2203"           # 선물차트
    FUTURES_TIME_CHART = "t2202"           # 선물시간대별체결
    FUTURES_TICK_DATA = "t2201"            # 선물틱조회
    FUTURES_INVESTOR_INFO = "t2401"        # 선물투자자별매매동향
    FUTURES_OPEN_INTEREST = "t2501"        # 선물미결제약정
    FUTURES_BASIS = "t2601"                # 선물베이시스
    
    # === OPTIONS MARKET DATA ===
    OPTIONS_MASTER_LIST = "t2301"          # 옵션종목정보조회 (gubun으로 구분)
    OPTIONS_CURRENT_PRICE = "t2105"        # 옵션현재가
    OPTIONS_CHAIN = "t2110"                # 옵션연쇄조회
    OPTIONS_THEORETICAL_PRICE = "t2106"    # 옵션이론가
    OPTIONS_CHART_DATA = "t2203"           # 옵션차트
    OPTIONS_TIME_CHART = "t2202"           # 옵션시간대별체결
    OPTIONS_TICK_DATA = "t2201"            # 옵션틱조회
    OPTIONS_GREEKS = "t2107"               # 옵션Greeks
    OPTIONS_IMPLIED_VOLATILITY = "t2108"    # 옵션내재변동성
    OPTIONS_PCR = "t2401"                  # Put/Call Ratio
    
    # === OVERSEAS FUTURES ===
    OVERSEAS_FUTURES_MASTER = "o3101"      # 해외선물종목정보
    OVERSEAS_FUTURES_PRICE = "o3103"       # 해외선물현재가
    OVERSEAS_FUTURES_CHART = "o3203"       # 해외선물차트
    OVERSEAS_FUTURES_TIME_CHART = "o3202"  # 해외선물시간대별체결
    
    # === ETF/ELW ===
    ETF_MASTER_LIST = "t1404"              # ETF종목정보 (gubun='E')
    ETF_CURRENT_PRICE = "t1101"            # ETF현재가 (주식과 동일)
    ETF_NAV_INFO = "t1481"                 # ETF NAV정보
    ETF_PORTFOLIO = "t1482"                # ETF구성종목
    ELW_MASTER_LIST = "t1404"              # ELW종목정보 (gubun='W')
    ELW_CURRENT_PRICE = "t1101"            # ELW현재가
    ELW_THEORETICAL_PRICE = "t1481"        # ELW이론가
    
    # === REAL-TIME DATA CODES ===
    # Stocks
    REALTIME_STOCK_PRICE = "S3_"           # 실시간주식체결
    REALTIME_STOCK_QUOTE = "H1_"           # 실시간주식호가
    REALTIME_STOCK_VI = "VI_"              # 실시간VI발동/해제
    
    # Futures
    REALTIME_FUTURES_PRICE = "FC_"         # 실시간선물체결
    REALTIME_FUTURES_QUOTE = "FH_"         # 실시간선물호가
    REALTIME_FUTURES_OI = "FO_"            # 실시간선물미결제약정
    
    # Options  
    REALTIME_OPTIONS_PRICE = "OC_"         # 실시간옵션체결
    REALTIME_OPTIONS_QUOTE = "OH_"         # 실시간옵션호가
    REALTIME_OPTIONS_OI = "OO_"            # 실시간옵션미결제약정
    
    # Overseas Futures
    REALTIME_OVERSEAS_FUTURES = "OF_"      # 실시간해외선물
    
    # === ACCOUNT & TRADING ===
    # Stock Account
    STOCK_ACCOUNT_BALANCE = "t0424"        # 주식잔고조회
    STOCK_ORDER = "CSPAT00600"             # 주식매수주문
    STOCK_SELL = "CSPAT00700"              # 주식매도주문
    
    # Futures Account  
    FUTURES_ACCOUNT_BALANCE = "t2424"      # 선물잔고조회
    FUTURES_ORDER = "CFOFAT00100"          # 선물신규주문
    FUTURES_LIQUIDATE = "CFOFAT00200"      # 선물청산주문
    
    # Options Account
    OPTIONS_ACCOUNT_BALANCE = "t2424"      # 옵션잔고조회  
    OPTIONS_ORDER = "CFOFAT00100"          # 옵션신규주문
    OPTIONS_LIQUIDATE = "CFOFAT00200"      # 옵션청산주문
    
    # Overseas Futures Account
    OVERSEAS_FUTURES_BALANCE = "o3424"     # 해외선물잔고
    OVERSEAS_FUTURES_ORDER = "CFOFAO0100"  # 해외선물주문

class DerivativesUtils:
    """Utilities for derivatives analysis and calculations"""
    
    @staticmethod
    def parse_futures_code(futures_code: str) -> Dict[str, str]:
        """
        Parse Korean futures code
        Example: '101RC000' -> {'underlying': 'KOSPI200', 'month': 'R', 'year': 'C'}
        """
        if len(futures_code) < 6:
            return {}
        
        # Korean futures code structure: AAABCD##
        underlying_map = {
            '101': 'KOSPI200',
            '105': 'KQ150',
            '106': 'KOSPI200Mini',
            '167': 'USD',
            '175': 'KTB3Y',
            '276': 'KTB10Y'
        }
        
        underlying_code = futures_code[:3]
        month_code = futures_code[3]
        year_code = futures_code[4]
        
        # Month mapping (선물 월물 코드)
        month_map = {
            'A': '01', 'B': '02', 'C': '03', 'D': '04',
            'E': '05', 'F': '06', 'G': '07', 'H': '08', 
            'I': '09', 'J': '10', 'K': '11', 'L': '12',
            'M': '01', 'N': '02', 'O': '03', 'P': '04',
            'Q': '05', 'R': '06', 'S': '07', 'T': '08',
            'U': '09', 'V': '10', 'W': '11', 'X': '12'
        }
        
        return {
            'underlying': underlying_map.get(underlying_code, 'Unknown'),
            'underlying_code': underlying_code,
            'month': month_map.get(month_code, 'Unknown'),
            'month_code': month_code,
            'year_code': year_code,
            'full_code': futures_code
        }
    
    @staticmethod
    def parse_options_code(options_code: str) -> Dict[str, str]:
        """
        Parse Korean options code
        Example: '201RC240' -> {'underlying': 'KOSPI200', 'type': 'Call', 'strike': 240}
        """
        if len(options_code) < 7:
            return {}
        
        # Options code structure: 2/3 + underlying + month + year + strike
        option_type = 'Call' if options_code.startswith('2') else 'Put'
        underlying_code = options_code[1:4]
        month_code = options_code[4]
        year_code = options_code[5]
        strike_code = options_code[6:]
        
        underlying_map = {
            '01': 'KOSPI200',
            '05': 'KQ150',
            '06': 'KOSPI200Mini'
        }
        
        # Convert strike price (need to multiply by appropriate factor)
        try:
            strike_price = int(strike_code) * 2.5 if underlying_code == '01' else int(strike_code)
        except ValueError:
            strike_price = 0
        
        return {
            'type': option_type,
            'underlying': underlying_map.get(underlying_code, 'Unknown'),
            'underlying_code': underlying_code,
            'month_code': month_code,
            'year_code': year_code,
            'strike_price': strike_price,
            'strike_code': strike_code,
            'full_code': options_code
        }
    
    @staticmethod
    def calculate_options_moneyness(spot_price: float, strike_price: float) -> Dict[str, float]:
        """Calculate options moneyness metrics"""
        if strike_price <= 0:
            return {}
        
        moneyness = spot_price / strike_price
        
        return {
            'moneyness': moneyness,
            'atm_distance': abs(moneyness - 1.0),
            'classification': (
                'ITM' if moneyness > 1.0 else 
                'ATM' if abs(moneyness - 1.0) < 0.02 else 
                'OTM'
            )
        }
    
    @staticmethod
    def get_futures_expiry_months(num_months: int = 12) -> List[Dict[str, str]]:
        """Get next N futures expiry months with codes"""
        current_date = datetime.now()
        expiry_months = []
        
        # Futures expire on 2nd Thursday of March, June, September, December
        expiry_base_months = [3, 6, 9, 12]
        
        for i in range(num_months):
            month_offset = i // 4
            year_offset = current_date.year + month_offset
            month = expiry_base_months[i % 4]
            
            # Find 2nd Thursday
            first_day = datetime(year_offset, month, 1)
            first_thursday = first_day + timedelta(days=(3 - first_day.weekday()) % 7)
            second_thursday = first_thursday + timedelta(days=7)
            
            # Month codes for futures
            month_codes = {3: 'H', 6: 'M', 9: 'U', 12: 'Z'}
            year_code = str(year_offset)[-1]  # Last digit of year
            
            expiry_months.append({
                'expiry_date': second_thursday.strftime('%Y-%m-%d'),
                'month': month,
                'year': year_offset,
                'month_code': month_codes[month],
                'year_code': year_code,
                'contract_code': f"{month_codes[month]}{year_code}"
            })
        
        return expiry_months
    
    @staticmethod
    def get_options_expiry_months(num_months: int = 12) -> List[Dict[str, str]]:
        """Get next N options expiry months"""
        # Options expire on 2nd Thursday of each month
        current_date = datetime.now()
        expiry_months = []
        
        for i in range(num_months):
            target_date = current_date + timedelta(days=30 * i)
            year = target_date.year
            month = target_date.month
            
            # Find 2nd Thursday
            first_day = datetime(year, month, 1)
            first_thursday = first_day + timedelta(days=(3 - first_day.weekday()) % 7)
            second_thursday = first_thursday + timedelta(days=7)
            
            # Options month codes (different from futures)
            month_codes = {
                1: 'A', 2: 'B', 3: 'C', 4: 'D', 5: 'E', 6: 'F',
                7: 'G', 8: 'H', 9: 'I', 10: 'J', 11: 'K', 12: 'L'
            }
            year_code = str(year)[-1]
            
            expiry_months.append({
                'expiry_date': second_thursday.strftime('%Y-%m-%d'),
                'month': month,
                'year': year,
                'month_code': month_codes[month],
                'year_code': year_code,
                'contract_code': f"{month_codes[month]}{year_code}"
            })
        
        return expiry_months

class MarketDataProcessor:
    """Process and analyze market data for all asset types"""
    
    @staticmethod
    def calculate_futures_basis(futures_price: float, spot_price: float, 
                              days_to_expiry: int, interest_rate: float = 0.03) -> Dict[str, float]:
        """Calculate futures basis and related metrics"""
        if spot_price <= 0 or days_to_expiry <= 0:
            return {}
        
        time_to_expiry = days_to_expiry / 365.0
        theoretical_price = spot_price * (1 + interest_rate * time_to_expiry)
        
        basis = futures_price - spot_price
        basis_percentage = (basis / spot_price) * 100
        premium_discount = futures_price - theoretical_price
        
        return {
            'basis': basis,
            'basis_percentage': basis_percentage,
            'theoretical_price': theoretical_price,
            'premium_discount': premium_discount,
            'annualized_basis': basis / time_to_expiry if time_to_expiry > 0 else 0
        }
    
    @staticmethod
    def calculate_put_call_ratio(call_volume: int, put_volume: int, 
                               call_oi: int, put_oi: int) -> Dict[str, float]:
        """Calculate Put/Call ratio metrics"""
        pcr_volume = put_volume / call_volume if call_volume > 0 else 0
        pcr_oi = put_oi / call_oi if call_oi > 0 else 0
        
        return {
            'pcr_volume': pcr_volume,
            'pcr_open_interest': pcr_oi,
            'total_volume': call_volume + put_volume,
            'total_open_interest': call_oi + put_oi,
            'market_sentiment': (
                'Bearish' if pcr_volume > 1.0 else 
                'Neutral' if 0.8 <= pcr_volume <= 1.2 else 
                'Bullish'
            )
        }
    
    @staticmethod
    def analyze_options_chain(options_data: List[Dict]) -> Dict:
        """Analyze options chain data"""
        if not options_data:
            return {}
        
        calls = [opt for opt in options_data if opt.get('option_type') == 'C']
        puts = [opt for opt in options_data if opt.get('option_type') == 'P']
        
        # Calculate max pain (strike with maximum open interest)
        strike_oi = {}
        for opt in options_data:
            strike = opt.get('strike_price', 0)
            oi = opt.get('open_interest', 0)
            if strike in strike_oi:
                strike_oi[strike] += oi
            else:
                strike_oi[strike] = oi
        
        max_pain_strike = max(strike_oi.keys(), key=lambda k: strike_oi[k]) if strike_oi else 0
        
        # Calculate total Greeks
        total_delta = sum(opt.get('delta', 0) for opt in options_data)
        total_gamma = sum(opt.get('gamma', 0) for opt in options_data)
        total_theta = sum(opt.get('theta', 0) for opt in options_data)
        total_vega = sum(opt.get('vega', 0) for opt in options_data)
        
        return {
            'total_calls': len(calls),
            'total_puts': len(puts),
            'max_pain_strike': max_pain_strike,
            'total_call_oi': sum(opt.get('open_interest', 0) for opt in calls),
            'total_put_oi': sum(opt.get('open_interest', 0) for opt in puts),
            'total_delta': total_delta,
            'total_gamma': total_gamma, 
            'total_theta': total_theta,
            'total_vega': total_vega,
            'strike_range': {
                'min': min(opt.get('strike_price', 0) for opt in options_data if opt.get('strike_price', 0) > 0),
                'max': max(opt.get('strike_price', 0) for opt in options_data)
            } if options_data else {}
        }

class DerivativesScreener:
    """Screen and filter derivatives based on various criteria"""
    
    @staticmethod
    def screen_liquid_options(options_df: pd.DataFrame, 
                            min_volume: int = 100, 
                            min_oi: int = 1000) -> pd.DataFrame:
        """Screen for liquid options"""
        if options_df.empty:
            return options_df
        
        liquid_options = options_df[
            (options_df['volume'] >= min_volume) | 
            (options_df['open_interest'] >= min_oi)
        ]
        
        return liquid_options.sort_values(['volume', 'open_interest'], ascending=False)
    
    @staticmethod
    def screen_near_expiry_futures(futures_df: pd.DataFrame, 
                                 days_threshold: int = 30) -> pd.DataFrame:
        """Screen futures expiring within threshold days"""
        if futures_df.empty or 'expiry_date' not in futures_df.columns:
            return futures_df
        
        current_date = datetime.now()
        futures_df['days_to_expiry'] = pd.to_datetime(futures_df['expiry_date']).apply(
            lambda x: (x - current_date).days
        )
        
        near_expiry = futures_df[
            (futures_df['days_to_expiry'] > 0) & 
            (futures_df['days_to_expiry'] <= days_threshold)
        ]
        
        return near_expiry.sort_values('days_to_expiry')
    
    @staticmethod
    def screen_high_iv_options(options_df: pd.DataFrame, 
                             iv_percentile: float = 0.8) -> pd.DataFrame:
        """Screen for high implied volatility options"""
        if options_df.empty or 'implied_volatility' not in options_df.columns:
            return options_df
        
        iv_threshold = options_df['implied_volatility'].quantile(iv_percentile)
        high_iv_options = options_df[options_df['implied_volatility'] >= iv_threshold]
        
        return high_iv_options.sort_values('implied_volatility', ascending=False)

# Popular Korean derivatives symbols
class KoreanDerivativesSymbols:
    """Popular Korean derivatives symbols and their mappings"""
    
    # KOSPI200 Futures (monthly contracts)
    KOSPI200_FUTURES = [
        '101QC000',  # Current month
        '101RC000',  # Next month  
        '101SC000',  # Next+1 month
        '101TC000'   # Next+2 month
    ]
    
    # KOSPI200 Mini Futures
    KOSPI200_MINI_FUTURES = [
        '106QC000',
        '106RC000', 
        '106SC000',
        '106TC000'
    ]
    
    # USD Futures
    USD_FUTURES = [
        '167QC000',
        '167RC000',
        '167SC000'
    ]
    
    # Popular ETFs
    POPULAR_ETFS = [
        '069500',  # KODEX 200
        '114800',  # KODEX 인버스
        '122630',  # KODEX 레버리지
        '251340',  # KODEX 코스닥150
        '229200',  # KODEX 코스닥150선물인버스
    ]
    
    # ELWs (examples)
    POPULAR_ELWS = [
        'ELW001',  # Example ELW codes
        'ELW002',
        'ELW003'
    ]
    
    @staticmethod
    def get_current_month_derivatives() -> Dict[str, List[str]]:
        """Get current month derivatives symbols"""
        # This would typically be dynamic based on current date
        # For now, returning static examples
        return {
            'kospi200_futures': ['101RC000'],
            'kospi200_options_calls': ['201RC240', '201RC250', '201RC260'],
            'kospi200_options_puts': ['301RC240', '301RC250', '301RC260'],
            'usd_futures': ['167RC000'],
            'etfs': KoreanDerivativesSymbols.POPULAR_ETFS[:5]
        }

# Usage example
def demonstrate_derivatives_utils():
    """Demonstrate derivatives utilities"""
    print("🔍 Korean Derivatives Analysis Demo")
    print("=" * 40)
    
    # Parse futures code
    futures_code = "101RC000"
    futures_info = DerivativesUtils.parse_futures_code(futures_code)
    print(f"Futures Code Analysis: {futures_code}")
    print(f"  Underlying: {futures_info['underlying']}")
    print(f"  Month: {futures_info['month']}")
    print(f"  Year Code: {futures_info['year_code']}")
    
    # Parse options code  
    options_code = "201RC240"
    options_info = DerivativesUtils.parse_options_code(options_code)
    print(f"\nOptions Code Analysis: {options_code}")
    print(f"  Type: {options_info['type']}")
    print(f"  Underlying: {options_info['underlying']}")
    print(f"  Strike Price: {options_info['strike_price']}")
    
    # Get expiry months
    futures_expiries = DerivativesUtils.get_futures_expiry_months(4)
    print(f"\nNext 4 Futures Expiry Dates:")
    for expiry in futures_expiries:
        print(f"  {expiry['expiry_date']}: {expiry['contract_code']}")
    
    # Current month symbols
    current_symbols = KoreanDerivativesSymbols.get_current_month_derivatives()
    print(f"\nCurrent Month Derivatives:")
    for asset_type, symbols in current_symbols.items():
        print(f"  {asset_type}: {symbols}")

if __name__ == "__main__":
    demonstrate_derivatives_utils()
