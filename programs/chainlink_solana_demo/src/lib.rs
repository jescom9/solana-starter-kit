use anchor_lang::prelude::*;
use anchor_lang::solana_program::clock::Clock;
use pyth_solana_receiver_sdk::price_update::{get_feed_id_from_hex, PriceUpdateV2};

declare_id!("41Np7rprA1XXuJ7k83PMh6e5adpyFkdJ2NPh1sGd72A9");

// Chainlink program ID on Devnet
pub const CHAINLINK_PROGRAM_ID: Pubkey = pubkey!("HEvSKofvBgfaexv23kMabbYqxasxU3mQ4ibBMEmJWHny");

#[program]
pub mod chainlink_solana_demo {
    use super::*;

    pub fn initialize_asset_registry(ctx: Context<InitializeAssetRegistry>) -> Result<()> {
        let registry = &mut ctx.accounts.asset_registry;
        registry.authority = ctx.accounts.authority.key();
        registry.assets = Vec::new();
        registry.risk_params = Vec::new();

        msg!(
            "Asset Registry initialized with authority: {}",
            registry.authority
        );
        Ok(())
    }

    pub fn add_asset(
        ctx: Context<ManageAssetRegistry>,
        id: u8,
        price: u64,
        decimals: u8,
        pyth_feed_id: String,
    ) -> Result<()> {
        let registry = &mut ctx.accounts.asset_registry;

        // Check if ID already exists
        if registry.assets.iter().any(|a| a.id == id) {
            return Err(ErrorCode::AssetAlreadyExists.into());
        }

        // Convert hex string to feed ID
        let feed_id = get_feed_id_from_hex(&pyth_feed_id)?;

        registry.assets.push(AssetInfo {
            id,
            price,
            decimals,
            pyth_feed_id: feed_id,
        });

        msg!(
            "Added asset: id={}, price={}, decimals={}, pyth_feed_id={}",
            id,
            price,
            decimals,
            pyth_feed_id
        );
        Ok(())
    }

    pub fn update_price_from_pyth(ctx: Context<UpdatePriceFromPyth>, asset_id: u8) -> Result<()> {
        let registry = &mut ctx.accounts.asset_registry;

        // Find asset
        let asset = registry
            .assets
            .iter_mut()
            .find(|a| a.id == asset_id)
            .ok_or(ErrorCode::AssetNotFound)?;

        // Get price from Pyth price update account
        let price_update = &ctx.accounts.price_update;
        let clock = Clock::get()?;
        let maximum_age: u64 = 300; // 5 minutes maximum age

        let price =
            price_update.get_price_no_older_than(&clock, maximum_age, &asset.pyth_feed_id)?;

        // Convert price to our standard format (price is already in the right scale)
        let new_price = if price.exponent < 0 {
            // If exponent is negative, we need to scale down
            (price.price as u64) / 10u64.pow((-price.exponent) as u32)
        } else {
            // If exponent is positive, scale up
            (price.price as u64) * 10u64.pow(price.exponent as u32)
        };

        let old_price = asset.price;
        asset.price = new_price;

        msg!(
            "Updated asset {} price: {} -> {} (pyth: {} * 10^{})",
            asset_id,
            old_price,
            new_price,
            price.price,
            price.exponent
        );

        Ok(())
    }

    pub fn update_asset_price(
        ctx: Context<ManageAssetRegistry>,
        id: u8,
        new_price: u64,
    ) -> Result<()> {
        let registry = &mut ctx.accounts.asset_registry;

        let asset = registry
            .assets
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or(ErrorCode::AssetNotFound)?;

        asset.price = new_price;

        msg!("Updated asset {} price to {}", id, new_price);
        Ok(())
    }

    pub fn add_risk_param(
        ctx: Context<ManageAssetRegistry>,
        asset_id_a: u8,
        asset_id_b: u8,
        risk_level: u8,
    ) -> Result<()> {
        let registry = &mut ctx.accounts.asset_registry;

        // Verify both assets exist
        if !registry.assets.iter().any(|a| a.id == asset_id_a) {
            return Err(ErrorCode::AssetNotFound.into());
        }
        if !registry.assets.iter().any(|a| a.id == asset_id_b) {
            return Err(ErrorCode::AssetNotFound.into());
        }

        // Check if pair already exists
        if registry.risk_params.iter().any(|p| {
            (p.asset_id_a == asset_id_a && p.asset_id_b == asset_id_b)
                || (p.asset_id_a == asset_id_b && p.asset_id_b == asset_id_a)
        }) {
            return Err(ErrorCode::RiskParamAlreadyExists.into());
        }

        registry.risk_params.push(PairRiskParam {
            asset_id_a,
            asset_id_b,
            risk_level,
        });

        msg!(
            "Added risk param: assets {}-{}, level={}",
            asset_id_a,
            asset_id_b,
            risk_level
        );
        Ok(())
    }

    // ========== OBLIGATION INSTRUCTIONS ==========

    pub fn init_obligation(ctx: Context<InitObligation>) -> Result<()> {
        let obligation = &mut ctx.accounts.obligation;
        obligation.owner = ctx.accounts.owner.key();
        obligation.deposits = Vec::new();
        obligation.borrows = Vec::new();

        msg!("Obligation initialized for owner: {}", obligation.owner);
        Ok(())
    }

    pub fn add_deposit(ctx: Context<ModifyObligation>, asset_id: u8, amount: u64) -> Result<()> {
        let obligation = &mut ctx.accounts.obligation;
        let registry = &ctx.accounts.asset_registry;

        // Verify asset exists
        if !registry.assets.iter().any(|a| a.id == asset_id) {
            return Err(ErrorCode::AssetNotFound.into());
        }

        // Add or update deposit
        if let Some(position) = obligation
            .deposits
            .iter_mut()
            .find(|p| p.asset_id == asset_id)
        {
            position.amount = position
                .amount
                .checked_add(amount)
                .ok_or(ErrorCode::MathOverflow)?;
        } else {
            obligation.deposits.push(Position { asset_id, amount });
        }

        msg!("Added deposit: asset_id={}, amount={}", asset_id, amount);

        // Perform health check
        perform_health_check(ctx, asset_id, 0)?;

        Ok(())
    }

    pub fn add_borrow(ctx: Context<ModifyObligation>, asset_id: u8, amount: u64) -> Result<()> {
        let obligation = &mut ctx.accounts.obligation;
        let registry = &ctx.accounts.asset_registry;

        // Verify asset exists
        if !registry.assets.iter().any(|a| a.id == asset_id) {
            return Err(ErrorCode::AssetNotFound.into());
        }

        msg!("Adding borrow: asset_id={}, amount={}", asset_id, amount);
        msg!(
            "Current deposits: {}, borrows: {}",
            obligation.deposits.len(),
            obligation.borrows.len()
        );

        // Add or update borrow
        if let Some(position) = obligation
            .borrows
            .iter_mut()
            .find(|p| p.asset_id == asset_id)
        {
            position.amount = position
                .amount
                .checked_add(amount)
                .ok_or(ErrorCode::MathOverflow)?;
        } else {
            obligation.borrows.push(Position { asset_id, amount });
        }

        // Perform health check
        perform_health_check(ctx, 0, asset_id)?;

        Ok(())
    }

    pub fn remove_deposit(ctx: Context<ModifyObligation>, asset_id: u8, amount: u64) -> Result<()> {
        let obligation = &mut ctx.accounts.obligation;

        msg!("Removing deposit: asset_id={}, amount={}", asset_id, amount);
        msg!(
            "Current deposits: {}, borrows: {}",
            obligation.deposits.len(),
            obligation.borrows.len()
        );

        if amount == 0 {
            return Ok(());
        }

        let position = obligation
            .deposits
            .iter_mut()
            .find(|p| p.asset_id == asset_id)
            .ok_or(ErrorCode::DepositNotFound)?;

        if position.amount < amount {
            return Err(ErrorCode::InsufficientDeposit.into());
        }

        position.amount = position.amount.checked_sub(amount).unwrap();

        // Remove if zero
        if position.amount == 0 {
            obligation.deposits.retain(|p| p.asset_id != asset_id);
        }

        // Perform health check
        perform_health_check(ctx, asset_id, 0)?;

        Ok(())
    }

    pub fn remove_borrow(ctx: Context<ModifyObligation>, asset_id: u8, amount: u64) -> Result<()> {
        let obligation = &mut ctx.accounts.obligation;

        msg!("Removing borrow: asset_id={}, amount={}", asset_id, amount);

        if amount == 0 {
            return Ok(());
        }

        let position = obligation
            .borrows
            .iter_mut()
            .find(|p| p.asset_id == asset_id)
            .ok_or(ErrorCode::BorrowNotFound)?;

        if position.amount < amount {
            return Err(ErrorCode::InsufficientBorrow.into());
        }

        position.amount = position.amount.checked_sub(amount).unwrap();

        // Remove if zero
        if position.amount == 0 {
            obligation.borrows.retain(|p| p.asset_id != asset_id);
        }

        // Perform health check
        perform_health_check(ctx, 0, asset_id)?;

        Ok(())
    }

    // ========== DEBUG INSTRUCTION ==========

    pub fn debug_read_all_data(ctx: Context<DebugReadData>) -> Result<()> {
        let registry = &ctx.accounts.asset_registry;
        let obligation = &ctx.accounts.obligation;

        msg!("=== ASSET REGISTRY DATA ===");
        msg!("Authority: {}", registry.authority);
        msg!("Total assets: {}", registry.assets.len());

        for asset in &registry.assets {
            msg!(
                "Asset: id={}, price={}, decimals={}, pyth_feed_id={:?}",
                asset.id,
                asset.price,
                asset.decimals,
                asset.pyth_feed_id
            );
        }

        msg!("Total risk params: {}", registry.risk_params.len());
        for param in &registry.risk_params {
            msg!(
                "Risk param: {}-{}, level={}",
                param.asset_id_a,
                param.asset_id_b,
                param.risk_level
            );
        }

        msg!("=== OBLIGATION DATA ===");
        msg!("Owner: {}", obligation.owner);
        msg!("Deposits: {}", obligation.deposits.len());
        for deposit in &obligation.deposits {
            msg!(
                "  Deposit: asset_id={}, amount={}",
                deposit.asset_id,
                deposit.amount
            );
        }

        msg!("Borrows: {}", obligation.borrows.len());
        for borrow in &obligation.borrows {
            msg!(
                "  Borrow: asset_id={}, amount={}",
                borrow.asset_id,
                borrow.amount
            );
        }

        Ok(())
    }

    pub fn delete_obligation(_ctx: Context<DeleteObligation>) -> Result<()> {
        msg!(
            "Obligation for owner {} deleted",
            _ctx.accounts.obligation.owner
        );
        Ok(())
    }

    pub fn delete_asset_registry(_ctx: Context<DeleteAssetRegistry>) -> Result<()> {
        msg!("Asset registry deleted");
        Ok(())
    }

    pub fn demonstrate_oracle_price_pull(
        ctx: Context<DemonstrateOraclePrices>,
        asset_id: u8,
    ) -> Result<()> {
        let registry = &ctx.accounts.asset_registry;

        // Find asset
        let asset = registry
            .assets
            .iter()
            .find(|a| a.id == asset_id)
            .ok_or(ErrorCode::AssetNotFound)?;

        msg!("=== PYTH ORACLE PRICE DEMONSTRATION ===");
        msg!("Asset ID: {}", asset_id);
        msg!("Hardcoded price: ${}", asset.price);
        msg!("Pyth feed ID: {:?}", asset.pyth_feed_id);

        // Get price from Pyth price update account
        let price_update = &ctx.accounts.price_update;
        let clock = Clock::get()?;
        let maximum_age: u64 = 300; // 5 minutes maximum age

        let price =
            price_update.get_price_no_older_than(&clock, maximum_age, &asset.pyth_feed_id)?;

        // Convert price to our standard format
        let oracle_price = if price.exponent < 0 {
            (price.price as u64) / 10u64.pow((-price.exponent) as u32)
        } else {
            (price.price as u64) * 10u64.pow(price.exponent as u32)
        };

        msg!("Pyth raw price: {}", price.price);
        msg!("Pyth exponent: {}", price.exponent);
        msg!("Pyth confidence: {}", price.conf);
        msg!("Pyth price (converted): ${}", oracle_price);
        msg!(
            "Price difference: ${}",
            if oracle_price > asset.price {
                oracle_price - asset.price
            } else {
                asset.price - oracle_price
            }
        );
        msg!(
            "Price difference %: {}%",
            if asset.price > 0 {
                if oracle_price > asset.price {
                    ((oracle_price - asset.price) * 100) / asset.price
                } else {
                    ((asset.price - oracle_price) * 100) / asset.price
                }
            } else {
                0
            }
        );

        msg!("=== DEMONSTRATION COMPLETE ===");
        Ok(())
    }
}

// ========== HEALTH CHECK FUNCTION ==========

pub fn perform_health_check(
    ctx: Context<ModifyObligation>,
    _asset_a: u8,
    _asset_b: u8,
) -> Result<()> {
    let obligation = &mut ctx.accounts.obligation;
    let asset_registry = &ctx.accounts.asset_registry;
    let price_update = &ctx.accounts.price_update;

    msg!(
        "🔍 Starting health check for obligation with {} deposits and {} borrows",
        obligation.deposits.len(),
        obligation.borrows.len()
    );

    let mut total_deposit_value: u64 = 0;
    let mut total_borrow_value: u64 = 0;

    // Process deposits with real-time Pyth prices
    for deposit in &obligation.deposits {
        let asset_info = &asset_registry.assets[deposit.asset_id as usize];

        // Try to get real price from Pyth oracle
        let current_price = match get_pyth_price(price_update, &asset_info.pyth_feed_id) {
            Ok(price) => {
                msg!(
                    "📈 Real Pyth price for asset {}: ${:.2} (feed ID available)",
                    deposit.asset_id,
                    price as f64 / 100.0
                );
                price
            }
            Err(_) => {
                msg!(
                    "⚠️ Failed to get Pyth price for asset {}, using fallback price: ${:.2}",
                    deposit.asset_id,
                    asset_info.price as f64 / 100.0
                );
                asset_info.price
            }
        };

        let deposit_value = calculate_value(deposit.amount, current_price, asset_info.decimals);
        total_deposit_value = total_deposit_value.checked_add(deposit_value).unwrap();

        msg!(
            "💰 Deposit: Asset {} = {} units × ${:.2} = ${:.2}",
            deposit.asset_id,
            deposit.amount,
            current_price as f64 / 100.0,
            deposit_value as f64 / 100.0
        );
    }

    // Process borrows with real-time Pyth prices
    for borrow in &obligation.borrows {
        let asset_info = &asset_registry.assets[borrow.asset_id as usize];

        // Try to get real price from Pyth oracle
        let current_price = match get_pyth_price(price_update, &asset_info.pyth_feed_id) {
            Ok(price) => {
                msg!(
                    "📈 Real Pyth price for asset {}: ${:.2} (feed ID available)",
                    borrow.asset_id,
                    price as f64 / 100.0
                );
                price
            }
            Err(_) => {
                msg!(
                    "⚠️ Failed to get Pyth price for asset {}, using fallback price: ${:.2}",
                    borrow.asset_id,
                    asset_info.price as f64 / 100.0
                );
                asset_info.price
            }
        };

        let borrow_value = calculate_value(borrow.amount, current_price, asset_info.decimals);
        total_borrow_value = total_borrow_value.checked_add(borrow_value).unwrap();

        msg!(
            "🔴 Borrow: Asset {} = {} units × ${:.2} = ${:.2}",
            borrow.asset_id,
            borrow.amount,
            current_price as f64 / 100.0,
            borrow_value as f64 / 100.0
        );
    }

    // Calculate health score using real-time prices
    let health_score = if total_borrow_value == 0 {
        u64::MAX // Infinite health score if no borrows
    } else {
        total_deposit_value
            .checked_mul(1000)
            .unwrap()
            .checked_div(total_borrow_value)
            .unwrap()
    };

    obligation.health_score = health_score;

    msg!("📊 Health Check Results (Using Real Pyth Prices):");
    msg!(
        "   Total Deposit Value: ${:.2}",
        total_deposit_value as f64 / 100.0
    );
    msg!(
        "   Total Borrow Value: ${:.2}",
        total_borrow_value as f64 / 100.0
    );
    msg!("   Health Score: {} (minimum required: 1000)", health_score);

    if health_score < 1000 {
        msg!(
            "🚨 LIQUIDATION ALERT: Health score {} is below minimum 1000!",
            health_score
        );
        return Err(ErrorCode::InsufficientCollateral.into());
    } else {
        msg!("✅ Position is healthy with score: {}", health_score);
    }

    Ok(())
}

// Helper function to get price from Pyth PriceUpdateV2 account
fn get_pyth_price(price_update_account: &AccountInfo, feed_id: &[u8; 32]) -> Result<u64> {
    use pyth_solana_receiver_sdk::price_update::PriceUpdateV2;

    // Try to deserialize as PriceUpdateV2
    let price_update =
        match PriceUpdateV2::try_deserialize(&mut price_update_account.data.borrow().as_ref()) {
            Ok(update) => update,
            Err(_) => {
                msg!("⚠️ Failed to deserialize PriceUpdateV2 account");
                return Err(ErrorCode::InvalidPriceUpdate.into());
            }
        };

    // Get price with maximum age of 60 seconds
    let price_feed = price_update.get_price_no_older_than(
        &Clock::get()?,
        60, // Maximum age in seconds
        feed_id,
    )?;

    // Convert price to our format (price in cents)
    let price_scaled = if price_feed.exponent >= 0 {
        (price_feed.price as u64)
            .checked_mul(10_u64.pow(price_feed.exponent as u32))
            .unwrap_or(0)
    } else {
        (price_feed.price as u64)
            .checked_div(10_u64.pow((-price_feed.exponent) as u32))
            .unwrap_or(0)
    };

    // Convert to cents (multiply by 100)
    let price_in_cents = price_scaled.checked_mul(100).unwrap_or(0);

    msg!(
        "🔍 Pyth price details: price={}, exponent={}, scaled_price={}, final_price_cents={}",
        price_feed.price,
        price_feed.exponent,
        price_scaled,
        price_in_cents
    );

    Ok(price_in_cents)
}

// ========== CONTEXTS ==========

#[derive(Accounts)]
pub struct InitializeAssetRegistry<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + AssetRegistry::INIT_SPACE,
        seeds = [b"asset_registry"],
        bump
    )]
    pub asset_registry: Account<'info, AssetRegistry>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ManageAssetRegistry<'info> {
    #[account(
        mut,
        seeds = [b"asset_registry"],
        bump,
        has_one = authority
    )]
    pub asset_registry: Account<'info, AssetRegistry>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct InitObligation<'info> {
    #[account(
        init,
        payer = owner,
        space = 8 + Obligation::INIT_SPACE,
        seeds = [b"obligation", owner.key().as_ref()],
        bump
    )]
    pub obligation: Account<'info, Obligation>,
    #[account(mut)]
    pub owner: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ModifyObligation<'info> {
    #[account(
        mut,
        seeds = [b"obligation", owner.key().as_ref()],
        bump,
        has_one = owner
    )]
    pub obligation: Account<'info, Obligation>,
    #[account(
        seeds = [b"asset_registry"],
        bump
    )]
    pub asset_registry: Account<'info, AssetRegistry>,
    pub owner: Signer<'info>,
    /// CHECK: Pyth price update account for health check oracle prices
    pub price_update: AccountInfo<'info>,
}

#[derive(Accounts)]
pub struct DebugReadData<'info> {
    #[account(
        seeds = [b"asset_registry"],
        bump
    )]
    pub asset_registry: Account<'info, AssetRegistry>,
    #[account(
        seeds = [b"obligation", obligation.owner.as_ref()],
        bump
    )]
    pub obligation: Account<'info, Obligation>,
}

#[derive(Accounts)]
pub struct UpdatePriceFromPyth<'info> {
    #[account(
        mut,
        seeds = [b"asset_registry"],
        bump
    )]
    pub asset_registry: Account<'info, AssetRegistry>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub price_update: Account<'info, PriceUpdateV2>,
}

#[derive(Accounts)]
pub struct DeleteRiskParam<'info> {
    #[account(
        mut,
        seeds = [b"asset_registry"],
        bump,
        has_one = authority
    )]
    pub asset_registry: Account<'info, AssetRegistry>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct DeleteObligation<'info> {
    #[account(
        mut,
        seeds = [b"obligation", owner.key().as_ref()],
        bump,
        close = owner
    )]
    pub obligation: Account<'info, Obligation>,
    #[account(mut)]
    pub owner: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct DeleteAssetRegistry<'info> {
    #[account(
        mut,
        seeds = [b"asset_registry"],
        bump,
        close = authority
    )]
    pub asset_registry: Account<'info, AssetRegistry>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct DemonstrateOraclePrices<'info> {
    #[account(
        seeds = [b"asset_registry"],
        bump
    )]
    pub asset_registry: Account<'info, AssetRegistry>,
    pub price_update: Account<'info, PriceUpdateV2>,
}

// ========== ERROR CODES ==========

#[error_code]
pub enum ErrorCode {
    #[msg("Asset already exists with this ID")]
    AssetAlreadyExists,
    #[msg("Asset not found in registry")]
    AssetNotFound,
    #[msg("Risk parameter already exists for this pair")]
    RiskParamAlreadyExists,
    #[msg("Deposit not found in obligation")]
    DepositNotFound,
    #[msg("Borrow not found in obligation")]
    BorrowNotFound,
    #[msg("Insufficient deposit amount")]
    InsufficientDeposit,
    #[msg("Insufficient borrow amount")]
    InsufficientBorrow,
    #[msg("Obligation health score is below minimum threshold")]
    Unhealthy,
    #[msg("Math operation overflowed")]
    MathOverflow,
    // PYTH ORACLE ERROR CODES
    #[msg("Wrong Pyth feed ID provided for asset")]
    WrongOracleFeed,
    #[msg("Invalid Pyth price update accounts provided")]
    InvalidOracleAccounts,
    #[msg("Missing Pyth price for asset")]
    MissingOraclePrice,
    #[msg("Pyth price is too old")]
    PythPriceTooOld,
    #[msg("Insufficient collateral")]
    InsufficientCollateral,
    #[msg("Invalid price update")]
    InvalidPriceUpdate,
}

// ========== DATA STRUCTURES ==========

#[account]
#[derive(InitSpace)]
pub struct AssetRegistry {
    pub authority: Pubkey,
    #[max_len(20)]
    pub assets: Vec<AssetInfo>,
    #[max_len(50)]
    pub risk_params: Vec<PairRiskParam>,
}

#[derive(Debug, Clone, AnchorSerialize, AnchorDeserialize, PartialEq, Eq, InitSpace)]
pub struct AssetInfo {
    pub id: u8,
    pub price: u64,
    pub decimals: u8,
    pub pyth_feed_id: [u8; 32],
}

#[derive(Debug, Clone, AnchorSerialize, AnchorDeserialize, PartialEq, Eq, InitSpace)]
pub struct PairRiskParam {
    pub asset_id_a: u8,
    pub asset_id_b: u8,
    pub risk_level: u8,
}

#[derive(Debug, Clone, AnchorSerialize, AnchorDeserialize, PartialEq, Eq, InitSpace)]
pub struct Position {
    pub asset_id: u8,
    pub amount: u64,
}

#[account]
#[derive(InitSpace)]
pub struct Obligation {
    pub owner: Pubkey,
    #[max_len(11)]
    pub deposits: Vec<Position>,
    #[max_len(10)]
    pub borrows: Vec<Position>,
    pub health_score: u64,
}

// ========== DECIMAL HANDLING FUNCTIONS ==========

fn scale_amount(amount: u64, decimals: u8) -> u64 {
    amount.checked_mul(10u64.pow(decimals as u32)).unwrap_or(0)
}

fn unscale_amount(amount: u64, decimals: u8) -> u64 {
    amount.checked_div(10u64.pow(decimals as u32)).unwrap_or(0)
}

fn calculate_value(amount: u64, price: u64, decimals: u8) -> u64 {
    // Scale the amount to match the price's decimal places
    let scaled_amount = scale_amount(amount, decimals);
    // Multiply by price and divide by 10^decimals to maintain precision
    scaled_amount
        .checked_mul(price)
        .and_then(|v| v.checked_div(10u64.pow(decimals as u32)))
        .unwrap_or(0)
}
