import type { Asset, DexMeanPriceResponse } from '../../../shared/models/DexMeanPrice';
import type { PoolClient } from 'pg';
import type { TransactionPaginationType } from './PaginationService';
import { sqlDexMeanPrice } from '../models/dex/sqlDexMeanPrice.queries';

function parseAssetItem(s: string | undefined | null): Buffer {
  // For the sake of the query, we represent ADA as ('', '') instead of (NULL, NULL).
  // (see sqlDexMeanPrice.queries.sql for details)
  return Buffer.from(s ?? "", 'hex');
}

function serializeAsset(policyId: Buffer | null, assetName: Buffer | null): Asset {
  if (policyId === null && assetName === null) {
    return null;
  }
  if (policyId !== null && assetName !== null) {
    return {
      policyId: policyId.toString('hex'),
      assetName: assetName.toString('hex'),
    };
  }
  throw new Error('Invalid asset query response'); // should be unreachable
}

export async function dexMeanPrices(
  request: TransactionPaginationType & {
    dbTx: PoolClient;
    addresses: Buffer[];
    reverseMap: Map<string, Set<string>>;
    assetPairs: {asset1: Asset, asset2: Asset}[];
    limit: number;
  }
): Promise<DexMeanPriceResponse> {
  if (request.addresses.length === 0 || request.assetPairs.length === 0) return { meanPrices: [] };
  const meanPrices = await sqlDexMeanPrice.run({
    after_tx_id: (request.after?.tx_id ?? -1)?.toString(),
    until_tx_id: request.until.tx_id.toString(),
    addresses: request.addresses,
    policy_id1: request.assetPairs.map(pair => parseAssetItem(pair.asset1?.policyId)),
    asset_name1: request.assetPairs.map(pair => parseAssetItem(pair.asset1?.assetName)),
    policy_id2: request.assetPairs.map(pair => parseAssetItem(pair.asset2?.policyId)),
    asset_name2: request.assetPairs.map(pair => parseAssetItem(pair.asset2?.assetName)),
    limit: request.limit.toString(),
  }, request.dbTx);
  return {
    meanPrices: meanPrices.map(result => ({
      tx_hash: result.tx_hash.toString('hex'),
      address: [...(request.reverseMap.get(result.address.toString('hex')) ?? [])][0],
      asset1: serializeAsset(result.policy_id1, result.asset_name1),
      asset2: serializeAsset(result.policy_id2, result.asset_name2),
      amount1: result.amount1,
      amount2: result.amount2,
    })),
  };
}
