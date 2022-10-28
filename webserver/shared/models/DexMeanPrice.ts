import { Address } from "./Address";
import { Pagination } from "./common";
import { AssetName, PolicyId } from "./PolicyIdAssetMap";

export type Asset = {
  policyId: PolicyId;
  assetName: AssetName;
} | null;

/**
 * @example "2042352568679"
 */
type Amount = string; // uint64

export type DexMeanPrice = {
    tx_hash: string;
    address: Address;
    asset1: Asset;
    asset2: Asset;
    amount1: Amount;
    amount2: Amount;
}

export type DexMeanPriceRequest = {
  addresses: Address[],
  assetPairs: {asset1: Asset, asset2: Asset}[];
  /** Defaults to `DEX_PRICE_LIMIT.RESPONSE` */
  limit?: number;
} & Pagination;

export type DexMeanPriceResponse = {
  meanPrices: DexMeanPrice[];
};
