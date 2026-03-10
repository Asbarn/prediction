#!/usr/bin/env node
import * as cdk from 'aws-cdk-lib';
import { PredictionStack } from '../lib/prediction-stack';

const app = new cdk.App();

new PredictionStack(app, 'PredictionStack', {
  env: {
    account: process.env.CDK_DEFAULT_ACCOUNT!,
    region: 'us-east-1',
  },
  description: 'Prediction market arbitrage system infrastructure',
});
