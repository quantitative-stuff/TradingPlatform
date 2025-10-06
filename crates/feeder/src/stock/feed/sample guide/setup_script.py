#!/usr/bin/env python3
"""
Setup Script for LS Securities API System
=========================================

This script helps set up the LS Securities API system with all requirements.
"""

import os
import sys
import json
import subprocess
import sqlite3
from pathlib import Path

class LSSecuritiesSetup:
    def __init__(self):
        self.project_dir = Path.cwd()
        self.config_file = self.project_dir / "ls_config.json"
        self.requirements_file = self.project_dir / "requirements.txt"
        
    def print_banner(self):
        """Print setup banner"""
        print("=" * 60)
        print("🚀 LS Securities API System Setup")
        print("=" * 60)
        print()
    
    def check_python_version(self):
        """Check Python version"""
        print("🐍 Checking Python version...")
        
        if sys.version_info < (3, 7):
            print("❌ Python 3.7 or higher is required")
            print(f"   Current version: {sys.version}")
            return False
        
        print(f"✅ Python {sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}")
        return True
    
    def create_requirements_file(self):
        """Create requirements.txt file"""
        print("📦 Creating requirements.txt...")
        
        requirements = [
            "requests>=2.28.0",
            "websockets>=10.0",
            "pandas>=1.5.0",
            "sqlite3",  # Built-in, but listed for clarity
            "asyncio",  # Built-in, but listed for clarity
            "pytz>=2022.1",
            "python-dotenv>=0.19.0"
        ]
        
        with open(self.requirements_file, 'w') as f:
            f.write('\n'.join(requirements))
            f.write('\n')
        
        print(f"✅ Created {self.requirements_file}")
    
    def install_requirements(self):
        """Install Python requirements"""
        print("📦 Installing Python packages...")
        
        try:
            # Check if pip is available
            subprocess.run([sys.executable, "-m", "pip", "--version"], 
                         check=True, capture_output=True)
            
            # Install requirements
            result = subprocess.run([
                sys.executable, "-m", "pip", "install", "-r", str(self.requirements_file)
            ], capture_output=True, text=True)
            
            if result.returncode == 0:
                print("✅ All packages installed successfully")
                return True
            else:
                print(f"❌ Package installation failed: {result.stderr}")
                return False
                
        except subprocess.CalledProcessError:
            print("❌ pip is not available")
            return False
        except Exception as e:
            print(f"❌ Installation failed: {e}")
            return False
    
    def create_config_file(self):
        """Create configuration file"""
        print("⚙️  Creating configuration file...")
        
        if self.config_file.exists():
            print(f"⚠️  Configuration file already exists: {self.config_file}")
            overwrite = input("   Overwrite? (y/N): ").strip().lower()
            if overwrite != 'y':
                print("   Skipping configuration file creation")
                return True
        
        # Get user input for configuration
        print("\n📝 Please provide your LS Securities API credentials:")
        print("   (You can get these from https://openapi.ls-sec.co.kr/)")
        
        app_key = input("   APP_KEY: ").strip()
        if not app_key:
            print("❌ APP_KEY is required")
            return False
        
        app_secret = input("   APP_SECRET: ").strip()
        if not app_secret:
            print("❌ APP_SECRET is required")
            return False
        
        # Ask about production vs mock
        print("\n🔧 Environment Setup:")
        print("   1. Mock Trading (for testing)")
        print("   2. Production Trading (real money!)")
        
        env_choice = input("   Choose environment (1-2): ").strip()
        is_production = env_choice == "2"
        
        if is_production:
            print("⚠️  WARNING: Production mode selected - real money will be involved!")
            confirm = input("   Are you sure? (yes/no): ").strip().lower()
            if confirm != "yes":
                is_production = False
                print("   Switched to mock trading mode")
        
        # Create configuration
        config = {
            "APP_KEY": app_key,
            "APP_SECRET": app_secret,
            "IS_PRODUCTION": is_production,
            "DB_PATH": "stock_master.db",
            "LOG_LEVEL": "INFO"
        }
        
        # Save configuration
        with open(self.config_file, 'w', encoding='utf-8') as f:
            json.dump(config, f, indent=2, ensure_ascii=False)
        
        print(f"✅ Configuration saved to {self.config_file}")
        
        if is_production:
            print("⚠️  IMPORTANT: Keep your credentials secure!")
            print("   Consider using environment variables for production")
        
        return True
    
    def create_env_file(self):
        """Create .env file template"""
        print("🔐 Creating .env file template...")
        
        env_file = self.project_dir / ".env"
        
        env_content = """# LS Securities API Configuration
# Copy this file and rename to .env, then fill in your credentials

LS_APP_KEY=your_app_key_here
LS_APP_SECRET=your_app_secret_here
LS_PRODUCTION=false
LS_DB_PATH=stock_master.db
LS_LOG_LEVEL=INFO
"""
        
        with open(env_file, 'w') as f:
            f.write(env_content)
        
        print(f"✅ Created {env_file}")
        print("   You can use this instead of the JSON config file")
    
    def create_database(self):
        """Create initial database"""
        print("🗄️  Creating database...")
        
        db_path = self.project_dir / "stock_master.db"
        
        try:
            conn = sqlite3.connect(db_path)
            cursor = conn.cursor()
            
            # Create tables
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
                    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )
            ''')
            
            cursor.execute('''
                CREATE TABLE IF NOT EXISTS realtime_data (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    stock_code TEXT,
                    current_price INTEGER,
                    change_price INTEGER,
                    change_rate REAL,
                    volume INTEGER,
                    trading_value INTEGER,
                    high_price INTEGER,
                    low_price INTEGER,
                    open_price INTEGER,
                    timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    FOREIGN KEY (stock_code) REFERENCES stock_master (stock_code)
                )
            ''')
            
            # Create indexes
            cursor.execute('CREATE INDEX IF NOT EXISTS idx_stock_code ON realtime_data(stock_code)')
            cursor.execute('CREATE INDEX IF NOT EXISTS idx_timestamp ON realtime_data(timestamp)')
            
            conn.commit()
            conn.close()
            
            print(f"✅ Database created: {db_path}")
            return True
            
        except Exception as e:
            print(f"❌ Database creation failed: {e}")
            return False
    
    def create_project_structure(self):
        """Create project directory structure"""
        print("📁 Creating project structure...")
        
        directories = [
            "exports",      # For data exports
            "logs",         # For log files
            "backups",      # For database backups
            "scripts"       # For custom scripts
        ]
        
        for directory in directories:
            dir_path = self.project_dir / directory
            dir_path.mkdir(exist_ok=True)
            print(f"   ✅ Created {directory}/")
        
        # Create .gitignore
        gitignore_content = """# LS Securities API
ls_config.json
.env
*.db
*.db-journal

# Data and logs
exports/
logs/
backups/

# Python
__pycache__/
*.py[cod]
*$py.class
*.so
.Python
build/
develop-eggs/
dist/
downloads/
eggs/
.eggs/
lib/
lib64/
parts/
sdist/
var/
wheels/
*.egg-info/
.installed.cfg
*.egg

# IDEs
.vscode/
.idea/
*.swp
*.swo
*~

# OS
.DS_Store
Thumbs.db
"""
        
        gitignore_file = self.project_dir / ".gitignore"
        with open(gitignore_file, 'w') as f:
            f.write(gitignore_content)
        
        print(f"✅ Created .gitignore")
    
    def create_startup_script(self):
        """Create startup script"""
        print("🚀 Creating startup script...")
        
        startup_script = """#!/usr/bin/env python3
\"\"\"
Quick Start Script for LS Securities API
\"\"\"

from ls_usage_example import LSSecuritiesExample
import asyncio

async def main():
    example = LSSecuritiesExample()
    await example.run_complete_example()

if __name__ == "__main__":
    asyncio.run(main())
"""
        
        script_file = self.project_dir / "start.py"
        with open(script_file, 'w') as f:
            f.write(startup_script)
        
        print(f"✅ Created start.py")
        print("   Run 'python start.py' to start the system")
    
    def print_next_steps(self):
        """Print next steps for the user"""
        print("\n" + "=" * 60)
        print("🎉 Setup Complete!")
        print("=" * 60)
        print()
        print("📋 Next Steps:")
        print()
        print("1. 🔑 Get LS Securities API credentials:")
        print("   - Visit: https://openapi.ls-sec.co.kr/")
        print("   - Apply for API access")
        print("   - Get your APP_KEY and APP_SECRET")
        print()
        
        if not self.config_file.exists():
            print("2. ⚙️  Configure the system:")
            print(f"   - Edit {self.config_file}")
            print("   - Add your APP_KEY and APP_SECRET")
            print()
        
        print("3. 🧪 Test the system:")
        print("   python start.py")
        print()
        print("4. 📖 Learn more:")
        print("   - Read the API documentation")
        print("   - Check example usage in ls_usage_example.py")
        print("   - Explore the utilities in ls_config_and_utils.py")
        print()
        print("⚠️  Important Notes:")
        print("   - Start with mock trading to test your setup")
        print("   - Keep your credentials secure")
        print("   - Be aware of API rate limits")
        print("   - Korean market hours: 09:00-15:30 KST")
        print()
        print("🆘 Need Help?")
        print("   - Check the LS Securities API documentation")
        print("   - Review the example code for common patterns")
        print()
    
    def run_setup(self):
        """Run the complete setup process"""
        self.print_banner()
        
        # Check Python version
        if not self.check_python_version():
            return False
        
        # Create project structure
        self.create_project_structure()
        
        # Create requirements file
        self.create_requirements_file()
        
        # Install packages
        if not self.install_requirements():
            print("⚠️  Package installation failed. You may need to install manually.")
        
        # Create configuration
        create_config = input("\n🔧 Create configuration file now? (Y/n): ").strip().lower()
        if create_config != 'n':
            if not self.create_config_file():
                print("⚠️  Configuration failed. You can create it later.")
        
        # Create .env template
        self.create_env_file()
        
        # Create database
        self.create_database()
        
        # Create startup script
        self.create_startup_script()
        
        # Print next steps
        self.print_next_steps()
        
        return True

def main():
    """Main setup function"""
    setup = LSSecuritiesSetup()
    
    if len(sys.argv) > 1 and sys.argv[1] == "--quick":
        # Quick setup without prompts
        print("🚀 Running quick setup...")
        setup.create_project_structure()
        setup.create_requirements_file()
        setup.create_env_file()
        setup.create_database()
        setup.create_startup_script()
        print("✅ Quick setup complete!")
        print("   Edit ls_config.json with your credentials and run 'python start.py'")
    else:
        # Interactive setup
        setup.run_setup()

if __name__ == "__main__":
    main()
