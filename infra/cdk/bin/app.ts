#!/usr/bin/env node
import * as cdk from 'aws-cdk-lib';
import { PredictionStack } from '../lib/prediction-stack';

const app = new cdk.App();

new PredictionStack(app, 'PredictionStack', {
  env: {
    account: '606103597377',
    region: 'us-east-1',
  },
  description: 'Prediction market arbitrage system infrastructure',
});
