"""
Comprehensive LS Securities Derivatives Trading System
====================================================

Complete example demonstrating:
1. Stock, Futures, Options, and ETF data management
2. Real-time monitoring across all asset types
3. Derivatives analysis and screening
4. Options chain analysis
5. Risk management for derivatives
6. Portfolio tracking across asset classes
"""

import asyncio
import pandas as pd
from datetime import datetime, timedelta
import json
import sqlite3
import numpy as np
from typing import Dict, List, Optional
import logging

# Assuming imports from our previous modules
# from ls_securities_enhanced_api import LSSecuritiesEnhancedAPI, AssetType
# from derivatives_transaction_codes import (
#     DerivativesUtils, MarketDataProcessor, DerivativesScreener, 
#     KoreanDerivativesSymbols, LSEnhancedTransactionCodes
# )

class ComprehensiveDerivativesSystem:
    """Complete derivatives trading system with analysis capabilities"""
    
    def __init__(self, config_file="ls_config.json"):
        """Initialize the comprehensive system"""
        self.api = LSSecuritiesEnhancedAPI(
            app_key="YOUR_APP_KEY",  # Replace with actual key
            app_secret="YOUR_APP_SECRET",  # Replace with actual secret
            is_production=False
        )
        
        self.derivatives_utils = DerivativesUtils()
        self.market_processor = MarketDataProcessor()
        self.screener = DerivativesScreener()
        self.logger = logging.getLogger(__name__)
        
        # Track active positions across all asset types
        self.active_positions = {
            'stocks': {},
            'futures': {},
            'options': {},
            'etfs': {}
        }
        
    async def initialize_comprehensive_database(self):
        """Step 1: Initialize comprehensive master database"""
        print("🗄️  Initializing comprehensive derivatives database...")
        
        # Load all asset types
        tasks = [
            self._load_stocks_data(),
            self._load_futures_data(),
            self._load_options_data(),
            self._load_etf_data(),
            self._load_overseas_futures_data()
        ]
        
        await asyncio.gather(*tasks)
        
        # Generate summary
        summary = self.api.get_all_assets_summary()
        self._print_database_summary(summary)
    
    async def _load_stocks_data(self):
        """Load stock master data"""
        print("   📈 Loading stock master data...")
        self.api.get_stock_master_list()
        
    async def _load_futures_data(self):
        """Load futures master data"""
        print("   📊 Loading futures master data...")
        self.api.get_futures_master_list()
        
    async def _load_options_data(self):
        """Load options master data"""
        print("   📉 Loading options master data...")
        self.api.get_options_master_list()
        
    async def _load_etf_data(self):
        """Load ETF master data"""
        print("   📋 Loading ETF master data...")
        self.api.get_etf_master_list()
        
    async def _load_overseas_futures_data(self):
        """Load overseas futures data"""
        print("   🌍 Loading overseas futures data...")
        self.api.get_overseas_futures_master_list()
        
    def _print_database_summary(self, summary: Dict):
        """Print database summary"""
        print(f"\n✅ Database initialization complete!")
        print(f"📊 Asset Summary:")
        for asset_type, count in summary.items():
            if asset_type != 'realtime_data':
                print(f"   • {asset_type.replace('_', ' ').title()}: {count:,} instruments")
        
        if 'realtime_data' in summary:
            print(f"📡 Real-time data points: {sum(summary['realtime_data'].values()):,}")
    
    def analyze_kospi200_ecosystem(self):
        """Step 2: Analyze KOSPI200 derivatives ecosystem"""
        print("\n🎯 Analyzing KOSPI200 Derivatives Ecosystem")
        print("=" * 50)
        
        # Get KOSPI200 related derivatives
        kospi_derivatives = self.api.get_derivatives_by_underlying("KOSPI200")
        
        print(f"📈 KOSPI200 Futures: {len(kospi_derivatives['futures'])} contracts")
        print(f"📊 KOSPI200 Options: {len(kospi_derivatives['options'])} contracts")
        
        # Analyze futures by expiry
        if kospi_derivatives['futures']:
            futures_df = pd.DataFrame(kospi_derivatives['futures'])
            print(f"\n🗓️  Futures by Expiry Month:")
            
            for _, futures in futures_df.iterrows():
                parsed = self.derivatives_utils.parse_futures_code(futures['futures_code'])
                print(f"   • {futures['futures_code']}: {parsed['underlying']} "
                     f"({parsed['month']}/{parsed['year_code']}) - Exp: {futures['expiry_date']}")
        
        # Analyze options chain
        if kospi_derivatives['options']:
            self._analyze_options_chain_summary(kospi_derivatives['options'])
        
        return kospi_derivatives
    
    def _analyze_options_chain_summary(self, options_data: List[Dict]):
        """Analyze options chain summary"""
        options_df = pd.DataFrame(options_data)
        
        # Group by expiry and option type
        print(f"\n📊 Options Chain Analysis:")
        
        # Count by type
        calls = options_df[options_df['option_type'] == 'C']
        puts = options_df[options_df['option_type'] == 'P']
        
        print(f"   • Call Options: {len(calls):,}")
        print(f"   • Put Options: {len(puts):,}")
        
        # Strike price ranges
        if not options_df.empty and 'strike_price' in options_df.columns:
            min_strike = options_df['strike_price'].min()
            max_strike = options_df['strike_price'].max()
            print(f"   • Strike Range: {min_strike:.1f} - {max_strike:.1f}")
        
        # Expiry dates
        if 'expiry_date' in options_df.columns:
            unique_expiries = options_df['expiry_date'].nunique()
            print(f"   • Unique Expiry Dates: {unique_expiries}")
    
    async def monitor_derivatives_realtime(self, duration_minutes: int = 5):
        """Step 3: Monitor derivatives real-time data"""
        print(f"\n🔴 Starting Real-time Derivatives Monitoring ({duration_minutes} minutes)")
        print("=" * 60)
        
        # Define symbols to monitor across asset types
        monitoring_symbols = {
            'stocks': ['005930', '000660', '035420'],  # Samsung, SK Hynix, Naver
            'futures': ['101RC000', '106RC000'],       # KOSPI200, Mini
            'options': ['201RC240', '201RC250', '301RC240', '301RC250'],  # Calls & Puts
            'etf': ['069500', '114800']                # KODEX200, KODEX Inverse
        }
        
        print(f"📡 Monitoring symbols:")
        for asset_type, symbols in monitoring_symbols.items():
            print(f"   • {asset_type.title()}: {symbols}")
        
        # Start real-time monitoring
        monitoring_task = asyncio.create_task(
            self._realtime_derivatives_monitor(monitoring_symbols, duration_minutes)
        )
        
        try:
            await monitoring_task
        except asyncio.CancelledError:
            print("🛑 Real-time monitoring stopped")
    
    async def _realtime_derivatives_monitor(self, symbols_by_type: Dict, duration_minutes: int):
        """Internal real-time monitoring with derivatives analysis"""
        # Start WebSocket connection
        websocket_task = asyncio.create_task(
            self.api.connect_enhanced_websocket(symbols_by_type)
        )
        
        end_time = datetime.now() + timedelta(minutes=duration_minutes)
        last_update = datetime.now()
        
        while datetime.now() < end_time:
            await asyncio.sleep(10)  # Update every 10 seconds
            
            current_time = datetime.now()
            if (current_time - last_update).seconds >= 30:  # Detailed update every 30 seconds
                await self._print_realtime_summary()
                last_update = current_time
        
        # Cancel WebSocket
        websocket_task.cancel()
        try:
            await websocket_task
        except asyncio.CancelledError:
            pass
    
    async def _print_realtime_summary(self):
        """Print real-time data summary"""
        conn = sqlite3.connect(self.api.db_path)
        
        # Get latest data by asset type
        query = """
            SELECT asset_type, symbol, current_price, change_rate, volume, timestamp
            FROM realtime_data 
            WHERE timestamp > datetime('now', '-1 minute')
            ORDER BY timestamp DESC
        """
        
        recent_data = pd.read_sql_query(query, conn)
        conn.close()
        
        if not recent_data.empty:
            print(f"\n📊 Latest Market Data ({datetime.now().strftime('%H:%M:%S')}):")
            
            for asset_type in recent_data['asset_type'].unique():
                asset_data = recent_data[recent_data['asset_type'] == asset_type].head(3)
                print(f"   📈 {asset_type.title()}:")
                
                for _, row in asset_data.iterrows():
                    change_symbol = "+" if row['change_rate'] >= 0 else ""
                    print(f"      {row['symbol']}: {row['current_price']:.2f} "
                         f"({change_symbol}{row['change_rate']:.2f}%) "
                         f"Vol: {row['volume']:,}")
    
    def analyze_options_strategies(self):
        """Step 4: Analyze popular options strategies"""
        print("\n📊 Options Strategy Analysis")
        print("=" * 40)
        
        # Get current options data
        conn = sqlite3.connect(self.api.db_path)
        
        options_query = """
            SELECT om.*, rd.current_price, rd.implied_volatility, rd.delta, rd.gamma
            FROM options_master om
            LEFT JOIN realtime_data rd ON om.option_code = rd.symbol
            WHERE rd.timestamp > datetime('now', '-1 hour')
            ORDER BY om.strike_price
        """
        
        options_df = pd.read_sql_query(options_query, conn)
        conn.close()
        
        if options_df.empty:
            print("   ⚠️  No recent options data available")
            return
        
        # Analyze by strategy types
        self._analyze_covered_call_opportunities(options_df)
        self._analyze_protective_put_opportunities(options_df)
        self._analyze_straddle_opportunities(options_df)
        
    def _analyze_covered_call_opportunities(self, options_df: pd.DataFrame):
        """Analyze covered call opportunities"""
        calls = options_df[options_df['option_type'] == 'C'].copy()
        
        if calls.empty:
            return
        
        # Find OTM calls with high premium
        calls['premium_yield'] = calls['current_price'] / calls['strike_price'] * 100
        high_premium_calls = calls[
            (calls['premium_yield'] > 1.0) &  # > 1% premium
            (calls['delta'] < 0.3)             # OTM
        ].head(5)
        
        print(f"\n🎯 Covered Call Opportunities:")
        for _, call in high_premium_calls.iterrows():
            print(f"   • {call['option_code']}: Strike {call['strike_price']:.0f}, "
                 f"Premium: {call['current_price']:.2f} ({call['premium_yield']:.2f}%)")
    
    def _analyze_protective_put_opportunities(self, options_df: pd.DataFrame):
        """Analyze protective put opportunities"""
        puts = options_df[options_df['option_type'] == 'P'].copy()
        
        if puts.empty:
            return
        
        # Find ATM/OTM puts for protection
        protection_puts = puts[
            (puts['delta'] > -0.3) &  # Not too deep ITM
            (puts['delta'] < -0.1)    # Some protection value
        ].head(5)
        
        print(f"\n🛡️  Protective Put Opportunities:")
        for _, put in protection_puts.iterrows():
            print(f"   • {put['option_code']}: Strike {put['strike_price']:.0f}, "
                 f"Cost: {put['current_price']:.2f}, Delta: {put['delta']:.3f}")
    
    def _analyze_straddle_opportunities(self, options_df: pd.DataFrame):
        """Analyze straddle opportunities"""
        # Group by strike price to find matching calls and puts
        strikes = options_df['strike_price'].unique()
        
        print(f"\n⚡ Long Straddle Opportunities:")
        straddle_count = 0
        
        for strike in sorted(strikes):
            strike_options = options_df[options_df['strike_price'] == strike]
            calls = strike_options[strike_options['option_type'] == 'C']
            puts = strike_options[strike_options['option_type'] == 'P']
            
            if not calls.empty and not puts.empty:
                call_price = calls['current_price'].iloc[0]
                put_price = puts['current_price'].iloc[0]
                total_cost = call_price + put_price
                
                # High IV straddles
                avg_iv = (calls['implied_volatility'].iloc[0] + puts['implied_volatility'].iloc[0]) / 2
                
                if avg_iv > 20:  # High IV threshold
                    print(f"   • Strike {strike:.0f}: Total Cost {total_cost:.2f}, "
                         f"IV: {avg_iv:.1f}%")
                    straddle_count += 1
                    
                    if straddle_count >= 3:  # Limit output
                        break
    
    def perform_risk_analysis(self):
        """Step 5: Perform portfolio risk analysis"""
        print("\n⚠️  Portfolio Risk Analysis")
        print("=" * 35)
        
        # Analyze portfolio exposure across asset types
        self._analyze_portfolio_exposure()
        self._calculate_var_analysis()
        self._analyze_correlation_risk()
        
    def _analyze_portfolio_exposure(self):
        """Analyze portfolio exposure by asset type"""
        conn = sqlite3.connect(self.api.db_path)
        
        exposure_query = """
            SELECT 
                asset_type,
                COUNT(*) as positions,
                SUM(current_price * volume) as market_value
            FROM realtime_data 
            WHERE timestamp > datetime('now', '-1 hour')
            GROUP BY asset_type
        """
        
        exposure_df = pd.read_sql_query(exposure_query, conn)
        conn.close()
        
        if not exposure_df.empty:
            total_value = exposure_df['market_value'].sum()
            
            print(f"📊 Portfolio Exposure by Asset Type:")
            for _, row in exposure_df.iterrows():
                percentage = (row['market_value'] / total_value) * 100 if total_value > 0 else 0
                print(f"   • {row['asset_type'].title()}: "
                     f"{row['market_value']:,.0f} KRW ({percentage:.1f}%)")
        
    def _calculate_var_analysis(self):
        """Calculate Value at Risk analysis"""
        print(f"\n📈 Value at Risk Analysis:")
        
        # This is a simplified VaR calculation
        # In practice, you'd use more sophisticated models
        conn = sqlite3.connect(self.api.db_path)
        
        var_query = """
            SELECT symbol, asset_type, change_rate
            FROM realtime_data
            WHERE timestamp > datetime('now', '-24 hours')
            ORDER BY timestamp
        """
        
        var_data = pd.read_sql_query(var_query, conn)
        conn.close()
        
        if not var_data.empty:
            # Calculate 1-day 95% VaR
            daily_returns = var_data.groupby('symbol')['change_rate'].last()
            portfolio_return = daily_returns.mean()
            portfolio_vol = daily_returns.std()
            var_95 = portfolio_return - 1.645 * portfolio_vol
            
            print(f"   • 1-Day 95% VaR: {var_95:.2f}%")
            print(f"   • Portfolio Volatility: {portfolio_vol:.2f}%")
    
    def _analyze_correlation_risk(self):
        """Analyze correlation risk between assets"""
        print(f"\n🔗 Correlation Risk Analysis:")
        
        conn = sqlite3.connect(self.api.db_path)
        
        corr_query = """
            SELECT symbol, asset_type, change_rate, timestamp
            FROM realtime_data
            WHERE timestamp > datetime('now', '-24 hours')
            AND symbol IN ('005930', '101RC000', '069500')
            ORDER BY timestamp
        """
        
        corr_data = pd.read_sql_query(corr_query, conn)
        conn.close()
        
        if len(corr_data['symbol'].unique()) >= 2:
            # Pivot data for correlation calculation
            pivot_data = corr_data.pivot_table(
                index='timestamp', 
                columns='symbol', 
                values='change_rate'
            )
            
            correlation_matrix = pivot_data.corr()
            
            print(f"   • Asset Correlations:")
            for i in range(len(correlation_matrix)):
                for j in range(i+1, len(correlation_matrix)):
                    asset1 = correlation_matrix.index[i]
                    asset2 = correlation_matrix.columns[j]
                    corr_val = correlation_matrix.iloc[i, j]
                    print(f"     {asset1} vs {asset2}: {corr_val:.3f}")
    
    def generate_trading_signals(self):
        """Step 6: Generate trading signals"""
        print("\n🎯 Trading Signal Generation")
        print("=" * 35)
        
        self._generate_momentum_signals()
        self._generate_mean_reversion_signals()
        self._generate_volatility_signals()
        
    def _generate_momentum_signals(self):
        """Generate momentum-based signals"""
        conn = sqlite3.connect(self.api.db_path)
        
        momentum_query = """
            SELECT symbol, asset_type, change_rate, volume,
                   LAG(change_rate, 1) OVER (PARTITION BY symbol ORDER BY timestamp) as prev_change
            FROM realtime_data
            WHERE timestamp > datetime('now', '-2 hours')
            ORDER BY timestamp DESC
        """
        
        momentum_data = pd.read_sql_query(momentum_query, conn)
        conn.close()
        
        if not momentum_data.empty:
            # Simple momentum signal: price acceleration with volume confirmation
            momentum_data['momentum_signal'] = (
                (momentum_data['change_rate'] > 0) & 
                (momentum_data['change_rate'] > momentum_data['prev_change']) &
                (momentum_data['volume'] > momentum_data['volume'].median())
            )
            
            strong_momentum = momentum_data[momentum_data['momentum_signal']].head(5)
            
            print(f"📈 Momentum Signals:")
            for _, signal in strong_momentum.iterrows():
                print(f"   • BUY {signal['symbol']} ({signal['asset_type']}): "
                     f"Change: +{signal['change_rate']:.2f}%, Vol: {signal['volume']:,}")
    
    def _generate_mean_reversion_signals(self):
        """Generate mean reversion signals"""
        print(f"\n🔄 Mean Reversion Signals:")
        
        conn = sqlite3.connect(self.api.db_path)
        
        reversion_query = """
            SELECT symbol, asset_type, change_rate,
                   AVG(change_rate) OVER (PARTITION BY symbol ORDER BY timestamp 
                                         ROWS BETWEEN 9 PRECEDING AND CURRENT ROW) as avg_10_change
            FROM realtime_data
            WHERE timestamp > datetime('now', '-2 hours')
            ORDER BY timestamp DESC
        """
        
        reversion_data = pd.read_sql_query(reversion_query, conn)
        conn.close()
        
        if not reversion_data.empty:
            # Signal when current move is 2+ std deviations from recent average
            reversion_data['z_score'] = (
                (reversion_data['change_rate'] - reversion_data['avg_10_change']) / 
                reversion_data.groupby('symbol')['change_rate'].transform('std')
            )
            
            extreme_moves = reversion_data[abs(reversion_data['z_score']) > 2].head(3)
            
            for _, signal in extreme_moves.iterrows():
                direction = "SELL" if signal['z_score'] > 0 else "BUY"
                print(f"   • {direction} {signal['symbol']} ({signal['asset_type']}): "
                     f"Z-Score: {signal['z_score']:.2f}")
    
    def _generate_volatility_signals(self):
        """Generate volatility-based signals"""
        print(f"\n⚡ Volatility Signals:")
        
        conn = sqlite3.connect(self.api.db_path)
        
        vol_query = """
            SELECT symbol, asset_type, implied_volatility, current_price
            FROM realtime_data
            WHERE asset_type = 'options' 
            AND implied_volatility IS NOT NULL
            AND timestamp > datetime('now', '-1 hour')
            ORDER BY implied_volatility DESC
        """
        
        vol_data = pd.read_sql_query(vol_query, conn)
        conn.close()
        
        if not vol_data.empty:
            # High IV signals (potential volatility selling opportunities)
            high_iv = vol_data[vol_data['implied_volatility'] > vol_data['implied_volatility'].quantile(0.8)].head(3)
            
            for _, signal in high_iv.iterrows():
                print(f"   • SELL VOL {signal['symbol']}: IV {signal['implied_volatility']:.1f}%")
    
    def export_comprehensive_report(self):
        """Step 7: Export comprehensive analysis report"""
        print("\n💾 Exporting Comprehensive Report")
        print("=" * 40)
        
        timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
        
        # Export all master data
        conn = sqlite3.connect(self.api.db_path)
        
        # Export each asset type
        tables_to_export = [
            ('stock_master', 'stocks'),
            ('futures_master', 'futures'),
            ('options_master', 'options'),
            ('overseas_futures_master', 'overseas_futures'),
            ('etf_master', 'etfs'),
            ('realtime_data', 'realtime_data')
        ]
        
        for table_name, file_prefix in tables_to_export:
            try:
                df = pd.read_sql_query(f"SELECT * FROM {table_name}", conn)
                if not df.empty:
                    filename = f"exports/{file_prefix}_{timestamp}.csv"
                    df.to_csv(filename, index=False, encoding='utf-8-sig')
                    print(f"   ✅ Exported {file_prefix}: {len(df):,} records → {filename}")
            except Exception as e:
                print(f"   ❌ Export failed for {file_prefix}: {e}")
        
        conn.close()
        
        # Generate summary report
        self._generate_summary_report(timestamp)
        
    def _generate_summary_report(self, timestamp: str):
        """Generate executive summary report"""
        summary_file = f"exports/executive_summary_{timestamp}.json"
        
        summary = self.api.get_all_assets_summary()
        
        # Add analysis results
        report = {
            'timestamp': timestamp,
            'database_summary': summary,
            'analysis_date': datetime.now().isoformat(),
            'market_status': 'Open' if self._is_market_open() else 'Closed',
            'total_instruments': sum(v for k, v in summary.items() if k != 'realtime_data'),
            'recommendations': [
                'Monitor KOSPI200 derivatives for hedging opportunities',
                'High IV options present volatility selling opportunities', 
                'Momentum signals detected in select ETFs',
                'Consider portfolio rebalancing based on correlation analysis'
            ]
        }
        
        with open(summary_file, 'w', encoding='utf-8') as f:
            json.dump(report, f, indent=2, ensure_ascii=False)
        
        print(f"   📊 Executive summary → {summary_file}")
    
    def _is_market_open(self) -> bool:
        """Check if Korean market is open"""
        now = datetime.now()
        if now.weekday() >= 5:  # Weekend
            return False
        
        market_open = now.replace(hour=9, minute=0, second=0, microsecond=0)
        market_close = now.replace(hour=15, minute=30, second=0, microsecond=0)
        
        return market_open <= now <= market_close
    
    async def run_comprehensive_analysis(self):
        """Run complete comprehensive analysis workflow"""
        print("🚀 LS Securities Comprehensive Derivatives System")
        print("=" * 60)
        print(f"📅 Analysis Date: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
        print(f"🏛️  Market Status: {'🟢 Open' if self._is_market_open() else '🔴 Closed'}")
        print()
        
        try:
            # Step 1: Initialize database
            await self.initialize_comprehensive_database()
            
            # Step 2: Analyze KOSPI200 ecosystem
            kospi_data = self.analyze_kospi200_ecosystem()
            
            # Step 3: Monitor real-time (shorter duration for demo)
            await self.monitor_derivatives_realtime(duration_minutes=2)
            
            # Step 4: Options strategy analysis
            self.analyze_options_strategies()
            
            # Step 5: Risk analysis
            self.perform_risk_analysis()
            
            # Step 6: Trading signals
            self.generate_trading_signals()
            
            # Step 7: Export comprehensive report
            self.export_comprehensive_report()
            
            print("\n🎉 Comprehensive Analysis Complete!")
            print("📋 Key Outputs:")
            print("   • Master databases updated for all asset types")
            print("   • Real-time data collected and analyzed")
            print("   • Options strategies identified")
            print("   • Risk metrics calculated")
            print("   • Trading signals generated")
            print("   • Comprehensive reports exported")
            
        except Exception as e:
            print(f"\n❌ Analysis failed: {e}")
            import traceback
            traceback.print_exc()

# Quick start functions for derivatives
def quick_derivatives_overview():
    """Quick overview of derivatives market"""
    print("🔍 Quick Derivatives Market Overview")
    print("=" * 40)
    
    system = ComprehensiveDerivativesSystem()
    
    # Get KOSPI200 derivatives
    kospi_derivatives = system.api.get_derivatives_by_underlying("KOSPI200")
    
    print(f"📊 KOSPI200 Ecosystem:")
    print(f"   • Futures Contracts: {len(kospi_derivatives['futures'])}")
    print(f"   • Options Contracts: {len(kospi_derivatives['options'])}")
    
    # Show popular derivatives
    popular_symbols = KoreanDerivativesSymbols.get_current_month_derivatives()
    print(f"\n🔥 Current Month Active Derivatives:")
    for asset_type, symbols in popular_symbols.items():
        print(f"   • {asset_type.replace('_', ' ').title()}: {len(symbols)} contracts")

def quick_options_chain_analysis(underlying="KOSPI200", expiry_date="2024-12-12"):
    """Quick options chain analysis"""
    print(f"📊 Quick Options Chain Analysis")
    print(f"Underlying: {underlying}, Expiry: {expiry_date}")
    print("=" * 50)
    
    system = ComprehensiveDerivativesSystem()
    options_chain = system.api.get_options_chain(underlying, expiry_date)
    
    print(f"📈 Call Options: {len(options_chain['calls'])}")
    print(f"📉 Put Options: {len(options_chain['puts'])}")
    
    if options_chain['calls']:
        print(f"\nSample Call Options:")
        for i, call in enumerate(options_chain['calls'][:3]):
            print(f"   {i+1}. Strike {call['strike_price']}: {call['option_code']}")
    
    if options_chain['puts']:
        print(f"\nSample Put Options:")
        for i, put in enumerate(options_chain['puts'][:3]):
            print(f"   {i+1}. Strike {put['strike_price']}: {put['option_code']}")

async def main():
    """Main execution function"""
    print("🎯 LS Securities Comprehensive Derivatives System")
    print("Choose an option:")
    print("1. Run comprehensive analysis")
    print("2. Quick derivatives overview") 
    print("3. Quick options chain analysis")
    print("4. Initialize database only")
    
    choice = input("\nEnter choice (1-4): ").strip()
    
    if choice == "1":
        system = ComprehensiveDerivativesSystem()
        await system.run_comprehensive_analysis()
    elif choice == "2":
        quick_derivatives_overview()
    elif choice == "3":
        quick_options_chain_analysis()
    elif choice == "4":
        system = ComprehensiveDerivativesSystem()
        await system.initialize_comprehensive_database()
    else:
        print("Invalid choice")

if __name__ == "__main__":
    asyncio.run(main())