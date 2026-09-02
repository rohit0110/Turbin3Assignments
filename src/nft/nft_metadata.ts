import {
  createSignerFromKeypair,
  signerIdentity,
} from "@metaplex-foundation/umi";
import wallet from "/Users/icarus/.config/solana/id.json";
import { createUmi } from "@metaplex-foundation/umi-bundle-defaults";
import { irysUploader } from "@metaplex-foundation/umi-uploader-irys";

const umi = createUmi(
  process.env.SOLANA_RPC_URL ?? "https://api.devnet.solana.com",
);

const keypair = umi.eddsa.createKeypairFromSecretKey(new Uint8Array(wallet));
const signer = createSignerFromKeypair(umi, keypair);

umi.use(
  irysUploader({
    address: "https://devnet.irys.xyz/",
  }),
);

umi.use(signerIdentity(signer));

(async () => {
  try {
    //change the image uri to your image uri obtained from nft_image.ts
    const image =
      "https://gateway.irys.xyz/7GXiY68yVQ2iWAA4N9fwZtaan5GKPAsnmdi8ZCv7ePFA";

    //json scheme : https://www.metaplex.com/docs/smart-contracts/core/json-schema
    //change the metadata
    const metadata = {
  "name": "Eyespy",
  "description": "Eyespy Logo",
  "image": image,
  "external_url": "https://example.com",
  "attributes": [
    {
      "trait_type": "color",
      "value": "solana colors"
    },
  ],
  "properties": {
    "files": [
      {
        "uri": image,
        "type": "image/png"
      },
    ],
    "category": "image"
  }
}
    const myUri = await umi.uploader.uploadJson(metadata);
    console.log(`metadata uri: ${myUri} `);
  } catch (error) {
    console.log("error", error);
  }
})();
