import wallet from "/Users/icarus/.config/solana/id.json";

import {
  appendTransactionMessageInstructions,
  assertIsTransactionWithBlockhashLifetime,
  createKeyPairSignerFromBytes,
  createSolanaRpc,
  createSolanaRpcSubscriptions,
  createTransactionMessage,
  generateKeyPairSigner,
  sendAndConfirmTransactionFactory,
  setTransactionMessageFeePayerSigner,
  setTransactionMessageLifetimeUsingBlockhash,
  signTransactionMessageWithSigners,
} from "@solana/kit";
import {
  getInitializeMintInstruction,
  getMintSize,
  TOKEN_PROGRAM_ADDRESS,
} from "@solana-program/token";
import { getCreateAccountInstruction } from "@solana-program/system";

const rpc = createSolanaRpc("https://api.devnet.solana.com");

const rpcSubscriptions = createSolanaRpcSubscriptions(
  "wss://api.devnet.solana.com",
);

(async () => {
  try {

    const signer = await createKeyPairSignerFromBytes(new Uint8Array(wallet));
    const mint = await generateKeyPairSigner();
    const mintSize = BigInt(getMintSize());
    const rent = await rpc.getMinimumBalanceForRentExemption(mintSize).send();
    const {value: latestBlockhash} = await rpc.getLatestBlockhash().send();
    const sendAndConfirm = sendAndConfirmTransactionFactory({rpc, rpcSubscriptions});

    const msg = createTransactionMessage({version: 0})

    const msgWithPayer = setTransactionMessageFeePayerSigner(signer, msg);

    const msgWithLifetime = setTransactionMessageLifetimeUsingBlockhash(latestBlockhash, msgWithPayer);

    const txMsg = appendTransactionMessageInstructions(
      [
        getCreateAccountInstruction({
          payer: signer,
          newAccount: mint,
          lamports: rent,
          space: mintSize,
          programAddress: TOKEN_PROGRAM_ADDRESS,
        }),
        getInitializeMintInstruction({
          mint: mint.address,
          decimals: 0,
          mintAuthority: signer.address,
        }),
      ],
      msgWithLifetime,
    );

    const signedTx = await signTransactionMessageWithSigners(txMsg);
    assertIsTransactionWithBlockhashLifetime(signedTx);

    const signature = await sendAndConfirm(signedTx, {commitment: "confirmed"});
    console.log(`Mint Address: ${mint.address}, Minted token with signature: ${signature}`);

} catch (error) {
    console.log(error);
  }
})();