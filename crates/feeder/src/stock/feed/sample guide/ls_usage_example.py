"""
Complete Usage Example for LS Securities API System
==================================================

This example demonstrates how to:
1. Set up the system
2. Build master database
3. Retrieve real-time data
4. Query and analyze data
"""

import asyncio
import json
import pandas as pd
from datetime import datetime, timedelta
import time
import signal
import sys

# Import our custom classes (assuming they're in separate files)
# from ls_securities_api import LSSecuritiesAPI
# from ls_config_and_utils import LSConfig, LSUtils, LSTransactionCodes, LSMarketHours

class LSSecuritiesExample:
    """Complete example of LS Securities API usage"""
    
    def __init__(self, config_file="ls_config.json"):
        """Initialize with configuration"""
        try:
            self.config = LSConfig.from_file(config_file)
        except FileNotFoundError:
            print(f"Config file {config_file} not found. Using environment variables.")
            self.config = LSConfig.from_env()
        
        if not self.config.APP_KEY or not self.config.APP_SECRET:
            print("❌ APP_KEY and APP_SECRET are required!")
            print("Please set them in config file or environment variables:")
            print("  - LS_APP_KEY")
            print("  - LS_APP_SECRET")
            sys.exit(1)
        
        self.api = LSSecuritiesAPI(
            app_key=self.config.APP_KEY,
            app_secret=self.config.APP_SECRET,
            is_production=self.config.IS_PRODUCTION
        )
        
        self.utils = LSUtils()
        self.market = LSMarketHours()
        self.running = True
        
        # Setup signal handlers for graceful shutdown
        signal.signal(signal.SIGINT, self._signal_handler)
        signal.signal(signal.SIGTERM, self._signal_handler)
    
    def _signal_handler(self, signum, frame):
        """Handle shutdown signals"""
        print(f"\n🔄 Received signal {signum}, shutting down gracefully...")
        self.running = False
    
    def setup_master_database(self):
        """Step 1: Setup master database with stock information"""
        print("🗄️  Setting up master database...")
        
        # Get and store stock master data
        print("   📥 Fetching stock master list...")
        self.api.get_stock_master_list()
        
        # Verify data
        stock_df = self.api.get_stock_list_from_db()
        print(f"   ✅ Loaded {len(stock_df)} stocks into master database")
        
        # Show sample data
        if not stock_df.empty:
            print("\n📊 Sample stock data:")
            print(stock_df[['stock_code', 'stock_name', 'market_code']].head(10))
        
        return stock_df
    
    def analyze_stock_universe(self):
        """Step 2: Analyze the stock universe"""
        print("\n🔍 Analyzing stock universe...")
        
        stock_df = self.api.get_stock_list_from_db()
        
        if stock_df.empty:
            print("   ⚠️  No stocks found in database. Please run setup_master_database() first.")
            return
        
        # Market distribution
        market_dist = stock_df['market_code'].value_counts()
        print(f"\n📈 Market Distribution:")
        for market_code, count in market_dist.items():
            market_name = self.utils.get_market_code_name(market_code)
            print(f"   {market_name} ({market_code}): {count} stocks")
        
        # Show some popular stocks
        popular_codes = self.utils.get_popular_stocks()
        popular_stocks = stock_df[stock_df['stock_code'].isin(popular_codes)]
        
        print(f"\n⭐ Popular stocks in our database:")
        for _, stock in popular_stocks.iterrows():
            print(f"   {stock['stock_code']}: {stock['stock_name']}")
    
    def get_current_market_data(self, stock_codes=None):
        """Step 3: Get current market data for selected stocks"""
        print("\n💹 Getting current market data...")
        
        if stock_codes is None:
            stock_codes = self.utils.get_popular_stocks()[:5]  # Top 5 popular stocks
        
        current_data = []
        
        for stock_code in stock_codes:
            if not self.utils.validate_stock_code(stock_code):
                print(f"   ⚠️  Invalid stock code: {stock_code}")
                continue
            
            print(f"   📊 Fetching data for {stock_code}...")
            price_data = self.api.get_current_price(stock_code)
            
            if price_data and 't1101OutBlock' in price_data:
                data = price_data['t1101OutBlock']
                current_data.append({
                    'stock_code': stock_code,
                    'current_price': int(data.get('price', 0)),
                    'change': int(data.get('change', 0)),
                    'change_rate': float(data.get('chrate', 0.0)),
                    'volume': int(data.get('volume', 0)),
                    'high': int(data.get('high', 0)),
                    'low': int(data.get('low', 0)),
                    'timestamp': datetime.now()
                })
                
                # Print formatted data
                price = self.utils.format_price(int(data.get('price', 0)))
                change = int(data.get('change', 0))
                change_rate = float(data.get('chrate', 0.0))
                
                change_symbol = "+" if change >= 0 else ""
                print(f"      💰 Price: {price} ({change_symbol}{change}, {change_rate:+.2f}%)")
            
            time.sleep(0.2)  # Rate limiting
        
        return current_data
    
    def get_historical_data(self, stock_code="005930", days=7):
        """Step 4: Get historical data"""
        print(f"\n📈 Getting {days}-day historical data for {stock_code}...")
        
        historical_data = self.api.get_time_series_data(stock_code, period_days=days)
        
        if historical_data and 't1301OutBlock' in historical_data:
            print(f"   ✅ Retrieved historical data")
            
            # Process and display sample data
            data_points = historical_data['t1301OutBlock']
            if data_points:
                print(f"   📊 Sample data points ({len(data_points)} total):")
                for i, point in enumerate(data_points[:5]):  # Show first 5 points
                    time_str = self.utils.parse_ls_time(point.get('chetime', ''))
                    price = self.utils.format_price(int(point.get('price', 0)))
                    volume = self.utils.format_price(int(point.get('cvolume', 0)))
                    print(f"      {i+1}. {time_str}: {price} (Vol: {volume})")
        
        return historical_data
    
    async def start_realtime_monitoring(self, stock_codes=None, duration_minutes=5):
        """Step 5: Start real-time data monitoring"""
        print(f"\n🔴 Starting real-time monitoring for {duration_minutes} minutes...")
        
        if not self.market.is_market_open():
            print("   ⚠️  Market is currently closed")
            next_open = self.market.get_next_market_open()
            print(f"   ⏰ Next market open: {next_open}")
            print("   🔄 Continuing with demo mode...")
        
        if stock_codes is None:
            stock_codes = self.utils.get_popular_stocks()[:3]  # Monitor top 3 stocks
        
        print(f"   📡 Monitoring stocks: {stock_codes}")
        
        # Start real-time monitoring
        monitoring_task = asyncio.create_task(
            self._realtime_monitor_with_timeout(stock_codes, duration_minutes)
        )
        
        try:
            await monitoring_task
        except asyncio.CancelledError:
            print("   🛑 Real-time monitoring cancelled")
        except Exception as e:
            print(f"   ❌ Error in real-time monitoring: {e}")
    
    async def _realtime_monitor_with_timeout(self, stock_codes, duration_minutes):
        """Monitor real-time data with timeout"""
        end_time = datetime.now() + timedelta(minutes=duration_minutes)
        
        # Create a task for WebSocket connection
        websocket_task = asyncio.create_task(
            self.api.connect_websocket(stock_codes)
        )
        
        # Monitor for specified duration
        while datetime.now() < end_time and self.running:
            await asyncio.sleep(10)  # Check every 10 seconds
            
            # Show recent data from database
            recent_data = self.api.get_realtime_data_from_db(limit=10)
            if not recent_data.empty:
                latest_entry = recent_data.iloc[0]
                print(f"   📊 Latest: {latest_entry['stock_code']} - "
                     f"Price: {self.utils.format_price(latest_entry['current_price'])}, "
                     f"Change: {latest_entry['change_rate']:+.2f}%")
        
        # Cancel WebSocket task
        websocket_task.cancel()
        try:
            await websocket_task
        except asyncio.CancelledError:
            pass
    
    def analyze_collected_data(self):
        """Step 6: Analyze collected data"""
        print("\n📊 Analyzing collected data...")
        
        # Get database statistics
        from ls_config_and_utils import LSDatabaseUtils
        stats = LSDatabaseUtils.get_database_stats(self.config.DB_PATH)
        
        print(f"   📈 Database Statistics:")
        print(f"      • Total stocks: {stats['stock_count']}")
        print(f"      • Real-time data points: {stats['realtime_data_count']}")
        
        if stats['data_date_range']['min']:
            print(f"      • Data date range: {stats['data_date_range']['min']} to {stats['data_date_range']['max']}")
        
        if stats['most_active_stocks']:
            print(f"   🔥 Most active stocks:")
            for stock_code, count in stats['most_active_stocks'][:5]:
                print(f"      • {stock_code}: {count} data points")
        
        # Show recent price movements
        recent_data = self.api.get_realtime_data_from_db(limit=20)
        if not recent_data.empty:
            print(f"\n📈 Recent price movements:")
            for _, row in recent_data.head(5).iterrows():
                timestamp = pd.to_datetime(row['timestamp']).strftime('%H:%M:%S')
                price = self.utils.format_price(row['current_price'])
                change_rate = row['change_rate']
                print(f"      {timestamp} - {row['stock_code']}: {price} ({change_rate:+.2f}%)")
    
    def export_data(self, output_dir="exports"):
        """Step 7: Export data to files"""
        import os
        print(f"\n💾 Exporting data to {output_dir}/...")
        
        os.makedirs(output_dir, exist_ok=True)
        
        # Export stock master data
        stock_df = self.api.get_stock_list_from_db()
        if not stock_df.empty:
            stock_file = f"{output_dir}/stock_master_{datetime.now().strftime('%Y%m%d_%H%M%S')}.csv"
            stock_df.to_csv(stock_file, index=False, encoding='utf-8-sig')
            print(f"   ✅ Stock master data exported to {stock_file}")
        
        # Export real-time data
        realtime_df = self.api.get_realtime_data_from_db(limit=1000)
        if not realtime_df.empty:
            realtime_file = f"{output_dir}/realtime_data_{datetime.now().strftime('%Y%m%d_%H%M%S')}.csv"
            realtime_df.to_csv(realtime_file, index=False, encoding='utf-8-sig')
            print(f"   ✅ Real-time data exported to {realtime_file}")
    
    async def run_complete_example(self):
        """Run the complete example workflow"""
        print("🚀 Starting LS Securities API Complete Example")
        print("=" * 50)
        
        try:
            # Step 1: Setup master database
            self.setup_master_database()
            
            # Step 2: Analyze stock universe
            self.analyze_stock_universe()
            
            # Step 3: Get current market data
            current_data = self.get_current_market_data()
            
            # Step 4: Get historical data
            historical_data = self.get_historical_data()
            
            # Step 5: Real-time monitoring (for demo, just 2 minutes)
            await self.start_realtime_monitoring(duration_minutes=2)
            
            # Step 6: Analyze collected data
            self.analyze_collected_data()
            
            # Step 7: Export data
            self.export_data()
            
            print("\n✅ Complete example finished successfully!")
            
        except KeyboardInterrupt:
            print("\n🛑 Example interrupted by user")
        except Exception as e:
            print(f"\n❌ Example failed with error: {e}")
            import traceback
            traceback.print_exc()

# Quick start functions
def quick_price_check(stock_codes=["005930", "000660", "035420"]):
    """Quick function to check current prices"""
    print("🔍 Quick Price Check")
    print("-" * 20)
    
    example = LSSecuritiesExample()
    current_data = example.get_current_market_data(stock_codes)
    return current_data

def quick_realtime_demo(minutes=1):
    """Quick real-time data demo"""
    print(f"📡 Quick Real-time Demo ({minutes} minutes)")
    print("-" * 30)
    
    example = LSSecuritiesExample()
    asyncio.run(example.start_realtime_monitoring(duration_minutes=minutes))

# Main execution
async def main():
    """Main execution function"""
    print("LS Securities API System")
    print("Choose an option:")
    print("1. Run complete example")
    print("2. Quick price check")
    print("3. Real-time demo (1 minute)")
    print("4. Setup database only")
    
    choice = input("\nEnter choice (1-4): ").strip()
    
    if choice == "1":
        example = LSSecuritiesExample()
        await example.run_complete_example()
    elif choice == "2":
        quick_price_check()
    elif choice == "3":
        quick_realtime_demo(1)
    elif choice == "4":
        example = LSSecuritiesExample()
        example.setup_master_database()
    else:
        print("Invalid choice")

if __name__ == "__main__":
    asyncio.run(main())
