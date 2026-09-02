import { createUmi } from "@metaplex-foundation/umi-bundle-defaults";
import wallet from "/Users/icarus/.config/solana/id.json";
import {
  createSignerFromKeypair,
  publicKey,
  signerIdentity,
} from "@metaplex-foundation/umi";
import { fetchAsset, mplCore, update } from "@metaplex-foundation/mpl-core";
import { base58 } from "@metaplex-foundation/umi/serializers";

const umi = createUmi(
  process.env.SOLANA_RPC_URL ?? "https://api.devnet.solana.com",
);

const keypair = umi.eddsa.createKeypairFromSecretKey(new Uint8Array(wallet));
const signer = createSignerFromKeypair(umi, keypair);

umi.use(signerIdentity(signer));

umi.use(mplCore());

(async () => {
  try {
    //paste the asset address printed by nft_mint.ts
    const assetAddress = publicKey(
      "4u3Y3X5vrntu8kDBKvqMCodooxsvdrp9LXUoTqFtTDke",
    );

    //the new on-chain name from nft_metadata.ts
    const newName = "From thanks Class to EYESPY";

    const asset = await fetchAsset(umi, assetAddress);

    const tx = await update(umi, {
      asset,
      name: newName,
    }).sendAndConfirm(umi);

    const signature = base58.deserialize(tx.signature)[0];

    console.log(
      `signature ${signature} , asset : ${asset.publicKey}`,
    );
  } catch (e) {
    console.log(`error ${e}`);
  }
})();
