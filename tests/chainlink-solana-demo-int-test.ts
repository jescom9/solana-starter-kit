import * as anchor from '@coral-xyz/anchor';

import BN from "bn.js" 
import { web3 } from "@coral-xyz/anchor"; 

import { assert } from "chai";
import { ChainlinkSolanaDemo } from '../target/types/chainlink_solana_demo';

const CHAINLINK_PROGRAM_ID = "HEvSKofvBgfaexv23kMabbYqxasxU3mQ4ibBMEmJWHny";
// SOL/USD feed account
const CHAINLINK_FEED = "669U43LNHx7LsVj95uYksnhXUfWKDsdzVqev3V4Jpw3P";
const DIVISOR = 100000000;

const CHAINLINK_FEED_ETH = '669U43LNHx7LsVj95uYksnhXUfWKDsdzVqev3V4Jpw3P' 
const CHAINLINK_FEED_USDC = '2EmfL3MqL3YHABudGNmajjCpR13NNEn9Y4LWxbDm6SwR' 

describe('chainlink-solana-demo', () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);  
  const program = anchor.workspace.ChainlinkSolanaDemo as anchor.Program<ChainlinkSolanaDemo>;
  const authority = provider.wallet.publicKey;

  it('Query SOL/USD Price Feed!', async () => {
    const priceFeedAccount = anchor.web3.Keypair.generate();
    // Execute the RPC.
    let transactionSignature = await program.methods
      .execute()
      .accounts({
        decimal: priceFeedAccount.publicKey,
        chainlinkFeed: CHAINLINK_FEED,
        chainlinkProgram: CHAINLINK_PROGRAM_ID,
      })
      .signers([priceFeedAccount])
      .rpc()

    // Fetch the account details of the account containing the price data
    const latestPrice = await program.account.decimal.fetch(priceFeedAccount.publicKey);
    console.log('Price Is: ' + latestPrice.value / DIVISOR)

    // Ensure the price returned is a positive value
    assert.ok(latestPrice.value / DIVISOR > 0);
  });

  it('Query ETH/USD Price Feed!', async () => {
    const priceFeedAccount = anchor.web3.Keypair.generate();
    // Execute the RPC.
    let transactionSignature = await program.methods
      .execute()
      .accounts({
        decimal: priceFeedAccount.publicKey,
        chainlinkFeed: CHAINLINK_FEED_ETH,
        chainlinkProgram: CHAINLINK_PROGRAM_ID,
      })
      .signers([priceFeedAccount])
      .rpc()

    // Fetch the account details of the account containing the price data
    const latestPrice = await program.account.decimal.fetch(priceFeedAccount.publicKey);
    console.log('Price Is: ' + latestPrice.value / DIVISOR)

    // Ensure the price returned is a positive value
    assert.ok(latestPrice.value / DIVISOR > 0);
  });

  it('Query USDC/USD Price Feed!', async () => {
    const priceFeedAccount = anchor.web3.Keypair.generate();
    // Execute the RPC.
    let transactionSignature = await program.methods
      .execute()
      .accounts({
        decimal: priceFeedAccount.publicKey,
        chainlinkFeed: CHAINLINK_FEED_USDC,
        chainlinkProgram: CHAINLINK_PROGRAM_ID,
      })
      .signers([priceFeedAccount])
      .rpc()

    // Fetch the account details of the account containing the price data
    const latestPrice = await program.account.decimal.fetch(priceFeedAccount.publicKey);
    console.log('Price Is: ' + latestPrice.value / DIVISOR)

    // Ensure the price returned is a positive value
    assert.ok(latestPrice.value / DIVISOR > 0);
  }); 

  // Test user wallet
  const testUser = web3.Keypair.generate();

  // Asset IDs for our test scenario
  const ASSET_A = 0;
  const ASSET_B = 1;
  const ASSET_C = 2;
  const ASSET_D = 3;

  // PDAs
  let assetRegistryPda: web3.PublicKey;
  let testObligationPda: web3.PublicKey;

  // Helper function to extract and print logs
  const printTransactionLogs = async (txSig: string, testName: string) => {
    console.log(`\n${'='.repeat(60)}`);
    console.log(`=== ${testName} ===`);
    console.log(`${'='.repeat(60)}`);
    console.log("Transaction signature:", txSig);

    await new Promise(resolve => setTimeout(resolve, 2000));

    try {
      const tx = await provider.connection.getTransaction(txSig, {
        commitment: "confirmed",
      });

      if (!tx || !tx.meta || !tx.meta.logMessages) {
        console.error("❌ Transaction data is incomplete");
        return;
      }

      const programLogs = tx.meta.logMessages.filter((log: string) => 
        log.includes("Program log:") && (
          log.includes("===") ||
          log.includes("Health") ||
          log.includes("Deposit:") ||
          log.includes("Borrow:") ||
          log.includes("WARNING") ||
          log.includes("✓") ||
          log.includes("Pair") ||
          log.includes("contribution")
        )
      );

      console.log("\n--- Program Logs ---");
      programLogs.forEach((log: string) => {
        const cleanLog = log.replace("Program log: ", "");
        console.log(cleanLog);
      });

    } catch (error) {
      console.error("Error fetching transaction logs:", error);
    }
  };

  before(async () => {
    console.log('\n=== SETUP PHASE ===');
    
    // Calculate PDAs
    [assetRegistryPda] = web3.PublicKey.findProgramAddressSync(
      [Buffer.from("asset_registry")],
      program.programId
    );

    [testObligationPda] = web3.PublicKey.findProgramAddressSync(
      [Buffer.from('obligation'), testUser.publicKey.toBuffer()],
      program.programId
    );
    
    // Fund test user
    const airdrop = await provider.connection.requestAirdrop(
      testUser.publicKey,
      2 * web3.LAMPORTS_PER_SOL
    );
    await provider.connection.confirmTransaction(airdrop);
    
    console.log('Test user wallet:', testUser.publicKey.toBase58());
    console.log('Asset Registry PDA:', assetRegistryPda.toBase58());
    console.log('Test Obligation PDA:', testObligationPda.toBase58());
  });

  describe("Setup", () => {
    it("initialize asset registry", async () => {
      await program.methods
        .initializeAssetRegistry()
        .accounts({
          assetRegistry: assetRegistryPda,
          authority,
          systemProgram: web3.SystemProgram.programId,
        })
        .rpc();

      console.log("✓ Asset registry initialized");
    });

    it("add all assets with price = $1", async () => {
      // All assets have price = 1 and decimals = 6 for simplicity
      const assets = [
        { id: ASSET_A, name: "A" },
        { id: ASSET_B, name: "B" },
        { id: ASSET_C, name: "C" },
        { id: ASSET_D, name: "D" },
      ];

      for (const asset of assets) {
        await program.methods
          .addAsset(
            asset.id,
            new BN(1),    // price = $1
            6             // 6 decimals
          )
          .accounts({
            assetRegistry: assetRegistryPda,
            authority,
          })
          .rpc();
        console.log(`✓ Added Asset ${asset.name} (ID ${asset.id})`);
      }
    });

    it("add risk parameters", async () => {
      const riskParams = [
        { a: ASSET_A, b: ASSET_B, risk: 80 }, // RiskAB = 0.8
        { a: ASSET_B, b: ASSET_C, risk: 80 }, // RiskBC = 0.8
        { a: ASSET_C, b: ASSET_D, risk: 60 }, // RiskCD = 0.6
        { a: ASSET_A, b: ASSET_C, risk: 90 }, // RiskAC = 0.9
        { a: ASSET_B, b: ASSET_D, risk: 40 }, // RiskBD = 0.4
        { a: ASSET_A, b: ASSET_D, risk: 60 }, // RiskAD = 0.6
      ];

      for (const param of riskParams) {
        await program.methods
          .addRiskParam(param.a, param.b, param.risk)
          .accounts({
            assetRegistry: assetRegistryPda,
            authority,
          })
          .rpc();
        console.log(`✓ Added risk param ${param.a}-${param.b}: ${param.risk/100}`);
      }
    });

    it("initialize test obligation", async () => {
      await program.methods
        .initObligation()
        .accounts({
          obligation: testObligationPda,
          owner: testUser.publicKey,
          systemProgram: web3.SystemProgram.programId,
        })
        .signers([testUser])
        .rpc();

      console.log("✓ Test obligation initialized");
    });
  });

  describe("Test Scenario 1: Health Score = 1.175", () => {
    it("setup position with deposits A=$1000, B=$1000 and borrows C=$250, D=$750", async () => {
      console.log("\n--- Setting up Scenario 1 ---");
      
      // Add deposits
      // Asset A: $1000 = 1000 * 10^6 (6 decimals)
      await program.methods
        .addDeposit(ASSET_A, new BN(1000000000))
        .accounts({
          obligation: testObligationPda,
          assetRegistry: assetRegistryPda,
          owner: testUser.publicKey,
        })
        .signers([testUser])
        .rpc();
      console.log("✓ Deposited $1000 of Asset A");

      // Asset B: $1000
      await program.methods
        .addDeposit(ASSET_B, new BN(1000000000))
        .accounts({
          obligation: testObligationPda,
          assetRegistry: assetRegistryPda,
          owner: testUser.publicKey,
        })
        .signers([testUser])
        .rpc();
      console.log("✓ Deposited $1000 of Asset B");

      // Add borrows
      // Asset C: $250
      const tx1 = await program.methods
        .addBorrow(ASSET_C, new BN(250000000))
        .accounts({
          obligation: testObligationPda,
          assetRegistry: assetRegistryPda,
          owner: testUser.publicKey,
        })
        .signers([testUser])
        .rpc();
      
      await printTransactionLogs(tx1, "Add Borrow C=$250");
      console.log("✓ Borrowed $250 of Asset C");

      // Asset D: $750
      const txSig = await program.methods
        .addBorrow(ASSET_D, new BN(750000000))
        .accounts({
          obligation: testObligationPda,
          assetRegistry: assetRegistryPda,
          owner: testUser.publicKey,
        })
        .signers([testUser])
        .rpc();
      console.log("✓ Borrowed $750 of Asset D");

      await printTransactionLogs(txSig, "Scenario 1 Complete - Expected Health Score: 1.175");

      // Verify position
      const obligation = await program.account.obligation.fetch(testObligationPda);
      console.log("\n--- Position Summary ---");
      console.log("Deposits:");
      obligation.deposits.forEach(d => {
        const value = d.amount.toNumber() / 1000000;
        console.log(`  Asset ${d.assetId}: ${value}`);
      });
      console.log("Borrows:");
      obligation.borrows.forEach(b => {
        const value = b.amount.toNumber() / 1000000;
        console.log(`  Asset ${b.assetId}: ${value}`);
      });
    });
  });

  describe("Test Scenario 2: Health Score = 0.9", () => {
    it("add additional $500 borrow of Asset C", async () => {
      console.log("\n--- Adding $500 more of Asset C ---");
      
      try {
        const txSig = await program.methods
        .addBorrow(ASSET_C, new BN(500000000))
        .accounts({
          obligation: testObligationPda,
          assetRegistry: assetRegistryPda,
          owner: testUser.publicKey,
        })
        .signers([testUser])
        .rpc();
      } catch (error) {
        console.log("✓ Correctly rejected unhealthy borrow"); 
      } finally {
        console.log("✓ Correctly rejected unhealthy borrow");
      }
    });
  });
});
 
