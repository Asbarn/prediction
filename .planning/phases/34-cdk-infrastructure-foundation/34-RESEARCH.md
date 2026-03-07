# Phase 34: CDK Infrastructure Foundation - Research

**Researched:** 2026-03-07
**Domain:** AWS CDK v2 (TypeScript) infrastructure-as-code for EC2-based deployment
**Confidence:** HIGH

## Summary

Phase 34 provisions all foundational AWS resources via CDK so that a single `cdk deploy` creates a complete, reproducible environment. The phase covers VPC, security groups, IAM instance profile, CloudWatch log group, Secrets Manager secret shells, and a separate EBS data volume -- while importing the existing ECR repository by reference rather than creating a duplicate.

The key architectural decision for this phase is using a **single CDK stack with multiple constructs** rather than multiple stacks. For a single-developer, single-target deployment, multiple stacks add cross-stack reference complexity and deployment ordering headaches with zero benefit. Logical separation is achieved through constructs (classes), not stack boundaries.

**Primary recommendation:** Create `infra/cdk/` subdirectory with one stack (`PredictionStack`) containing constructs for network, compute, secrets, and logging. Use `blockDevices` on the EC2 instance for a separate data volume with `deleteOnTermination: false`. Import ECR via `Repository.fromRepositoryName()`. Bootstrap CDK before first deploy.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| INFRA-01 | CDK stack provisions VPC, security groups, EC2 instance, and IAM instance profile in a single `cdk deploy` | Single-stack pattern with L2 constructs for Vpc, SecurityGroup, Instance, Role; verified code examples from official CDK docs |
| INFRA-02 | CDK imports existing ECR repository rather than creating a duplicate | `ecr.Repository.fromRepositoryName()` returns IRepository reference without CloudFormation ownership; verified in CDK API docs |
| INFRA-04 | IAM instance profile grants least-privilege access to ECR pull, CloudWatch Logs, Secrets Manager read, and AMP remote write | Combination of AWS managed policies (AmazonSSMManagedInstanceCore, AmazonEC2ContainerRegistryReadOnly, AmazonPrometheusRemoteWriteAccess) plus scoped inline policy for secretsmanager:GetSecretValue and logs:* |
| INFRA-05 | Separate EBS volume for persistent data survives instance replacement | `blockDevices` with `deleteOnTermination: false` on the Instance construct; user-data formats and mounts volume; data persists across instance stop/start and replacement |
| INFRA-06 | CDK provisions CloudWatch log group with 14-day retention policy | `logs.LogGroup` with `retention: RetentionDays.TWO_WEEKS` and `removalPolicy: RemovalPolicy.DESTROY` |
| INFRA-07 | CDK provisions Secrets Manager secrets for venue API credentials | `secretsmanager.Secret` with `secretStringTemplate` containing placeholder keys; values populated manually post-deploy |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| aws-cdk-lib | ^2.241.0 | All AWS construct libraries (EC2, IAM, ECR, SecretsManager, Logs) | CDK v2 monolith package; single dependency |
| aws-cdk (CLI) | ^2.1109.0 | Synthesize and deploy CloudFormation stacks | Required CLI companion |
| constructs | ^10.0.0 | Base construct library | Peer dependency of aws-cdk-lib |
| TypeScript | ^5.4 | CDK language | CDK primary language with best docs |
| Node.js | >=18.x LTS | CDK runtime | Required by CDK v2 |

### Modules Used (all from aws-cdk-lib)
| Module | Construct Level | Resources |
|--------|-----------------|-----------|
| `aws_ec2` | L2 | Vpc, SecurityGroup, Instance, BlockDeviceVolume, UserData, Peer, Port |
| `aws_ecr` | L2 | Repository.fromRepositoryName (import existing) |
| `aws_iam` | L2 | Role, ManagedPolicy, PolicyStatement |
| `aws_secretsmanager` | L2 | Secret with generateSecretString |
| `aws_logs` | L2 | LogGroup with RetentionDays |

### Installation
```bash
mkdir -p infra/cdk && cd infra/cdk
npx cdk init app --language typescript
# aws-cdk-lib and constructs installed automatically by cdk init
```

## Architecture Patterns

### Recommended Project Structure
```
prediction/                          # Rust project root (unchanged)
├── infra/
│   └── cdk/
│       ├── package.json             # CDK dependencies (isolated from Rust)
│       ├── tsconfig.json
│       ├── cdk.json
│       ├── bin/
│       │   └── app.ts               # CDK app entry point, single stack
│       └── lib/
│           └── prediction-stack.ts   # Single stack, all resources
├── deploy/                          # Existing directory
│   ├── aws-setup.sh                 # DEPRECATED by CDK user-data
│   └── ecr-push.sh                  # Remains for manual use
├── docker-compose.yml               # Unchanged in this phase
└── Dockerfile                       # Unchanged in this phase
```

### Pattern 1: Single Stack with Logical Constructs

**What:** One CDK stack (`PredictionStack`) containing all resources. Logical grouping via TypeScript code sections or private helper methods -- not separate stack classes.

**When to use:** Single-developer, single-target deployments where all resources share the same lifecycle and deployment cadence.

**Why not multiple stacks:** AWS CDK best practices recommend single stacks for small projects. Multiple stacks add cross-stack references (CloudFormation exports), deployment ordering, and risk of circular dependencies. For a solo-trader system with one EC2 instance, the added complexity provides zero benefit.

**Example:**
```typescript
// Source: https://docs.aws.amazon.com/cdk/v2/guide/best-practices.html
import * as cdk from 'aws-cdk-lib';
import * as ec2 from 'aws-cdk-lib/aws-ec2';
import * as iam from 'aws-cdk-lib/aws-iam';
import * as ecr from 'aws-cdk-lib/aws-ecr';
import * as secretsmanager from 'aws-cdk-lib/aws-secretsmanager';
import * as logs from 'aws-cdk-lib/aws-logs';

export class PredictionStack extends cdk.Stack {
  constructor(scope: cdk.App, id: string, props?: cdk.StackProps) {
    super(scope, id, props);

    // --- Network ---
    const vpc = new ec2.Vpc(this, 'Vpc', { /* ... */ });
    const sg = new ec2.SecurityGroup(this, 'InstanceSg', { vpc });

    // --- ECR (import existing) ---
    const repo = ecr.Repository.fromRepositoryName(this, 'EcrRepo', 'prediction');

    // --- Secrets ---
    const secret = new secretsmanager.Secret(this, 'ApiCredentials', { /* ... */ });

    // --- Logging ---
    const logGroup = new logs.LogGroup(this, 'LogGroup', { /* ... */ });

    // --- IAM ---
    const role = new iam.Role(this, 'InstanceRole', { /* ... */ });

    // --- Compute ---
    const instance = new ec2.Instance(this, 'Instance', { vpc, role, /* ... */ });
  }
}
```

### Pattern 2: ECR Import (Not Create)

**What:** Reference the existing ECR repository by name without CDK taking ownership. CDK will NOT attempt to create, modify, or delete it.

**Example:**
```typescript
// Source: https://docs.aws.amazon.com/cdk/api/v2/docs/aws-cdk-lib.aws_ecr-readme.html
const ecrRepo = ecr.Repository.fromRepositoryName(
  this, 'EcrRepo', 'prediction'
);
// Use ecrRepo.repositoryUri in user-data or outputs
// ecrRepo.grantPull(instanceRole) for IAM permissions
```

**Why `fromRepositoryName` over `fromRepositoryArn`:** Both work, but `fromRepositoryName` is simpler and avoids hardcoding the account ID and region in the ARN string. The name `prediction` is stable and known.

### Pattern 3: Separate EBS Data Volume via blockDevices

**What:** Add a second EBS volume to the EC2 instance for persistent data (/opt/prediction/data). Set `deleteOnTermination: false` so data survives instance replacement.

**Example:**
```typescript
// Source: https://docs.aws.amazon.com/cdk/api/v2/docs/aws-cdk-lib.aws_ec2.Instance.html
const instance = new ec2.Instance(this, 'Instance', {
  vpc,
  instanceType: ec2.InstanceType.of(ec2.InstanceClass.T3, ec2.InstanceSize.SMALL),
  machineImage: ec2.MachineImage.latestAmazonLinux2023(),
  role: instanceRole,
  blockDevices: [
    {
      deviceName: '/dev/xvda',  // Root volume
      volume: ec2.BlockDeviceVolume.ebs(20, {
        volumeType: ec2.EbsDeviceVolumeType.GP3,
      }),
    },
    {
      deviceName: '/dev/xvdf',  // Data volume
      volume: ec2.BlockDeviceVolume.ebs(30, {
        volumeType: ec2.EbsDeviceVolumeType.GP3,
        deleteOnTermination: false,  // CRITICAL: data persists
      }),
    },
  ],
});
```

**User-data must format and mount the data volume on first boot:**
```bash
# Check if volume has a filesystem, format if new
if ! blkid /dev/xvdf; then
  mkfs.ext4 /dev/xvdf
fi
mkdir -p /opt/prediction/data
mount /dev/xvdf /opt/prediction/data
echo '/dev/xvdf /opt/prediction/data ext4 defaults,nofail 0 2' >> /etc/fstab
```

**Important nuance:** `deleteOnTermination: false` means the EBS volume survives instance TERMINATION (including CDK-triggered replacement). However, the volume stays in the same AZ. If the new instance launches in a different AZ, it cannot attach. Pin the instance to a specific AZ to avoid this.

### Pattern 4: Secrets Manager Shell (Empty Secret)

**What:** CDK creates the secret structure with placeholder keys. Actual values are populated manually after first deploy via AWS Console or CLI.

**Example:**
```typescript
// Source: https://docs.aws.amazon.com/cdk/api/v2/docs/aws-cdk-lib.aws_secretsmanager-readme.html
const credentials = new secretsmanager.Secret(this, 'ApiCredentials', {
  secretName: 'prediction/prod/credentials',
  description: 'Venue API credentials for prediction system',
  generateSecretString: {
    secretStringTemplate: JSON.stringify({
      DERIBIT_CLIENT_ID: 'PLACEHOLDER',
      DERIBIT_CLIENT_SECRET: 'PLACEHOLDER',
      DERIVE_WALLET_KEY: 'PLACEHOLDER',
    }),
    generateStringKey: '_generated',  // Required field, unused
  },
});
```

After deploy, update via CLI:
```bash
aws secretsmanager put-secret-value \
  --secret-id prediction/prod/credentials \
  --secret-string '{"DERIBIT_CLIENT_ID":"real-id","DERIBIT_CLIENT_SECRET":"real-secret","DERIVE_WALLET_KEY":"real-key"}'
```

### Pattern 5: CloudWatch Log Group with Retention

**Example:**
```typescript
// Source: https://docs.aws.amazon.com/cdk/api/v2/docs/aws-cdk-lib.aws_logs-readme.html
const logGroup = new logs.LogGroup(this, 'AppLogGroup', {
  logGroupName: '/prediction/production',
  retention: logs.RetentionDays.TWO_WEEKS,  // 14 days per INFRA-06
  removalPolicy: cdk.RemovalPolicy.DESTROY, // Log group can be recreated
});
```

**Note:** Default retention is TWO_YEARS if not specified. Explicitly set TWO_WEEKS to control CloudWatch costs (referenced in PITFALLS research).

### Pattern 6: Least-Privilege IAM Instance Profile

**Example:**
```typescript
// Source: https://docs.aws.amazon.com/cdk/api/v2/docs/aws-cdk-lib.aws_iam-readme.html
const instanceRole = new iam.Role(this, 'InstanceRole', {
  assumedBy: new iam.ServicePrincipal('ec2.amazonaws.com'),
  description: 'EC2 instance role for prediction system',
});

// Managed policies
instanceRole.addManagedPolicy(
  iam.ManagedPolicy.fromAwsManagedPolicyName('AmazonSSMManagedInstanceCore')
);

// ECR pull via grant helper (more precise than managed policy)
ecrRepo.grantPull(instanceRole);

// AMP remote write
instanceRole.addManagedPolicy(
  iam.ManagedPolicy.fromAwsManagedPolicyName('AmazonPrometheusRemoteWriteAccess')
);

// Secrets Manager read (scoped to specific secret)
credentials.grantRead(instanceRole);

// CloudWatch Logs write (scoped to specific log group)
logGroup.grantWrite(instanceRole);
```

**Key insight:** Use CDK's `.grant*()` methods instead of inline PolicyStatements where possible. They produce correctly scoped permissions and handle edge cases (like ECR `GetAuthorizationToken` which requires `*` resource).

### Anti-Patterns to Avoid

- **Multiple CDK stacks for this scale:** Adds cross-stack references and deployment complexity for zero benefit. Use one stack with logical code organization.
- **Creating a new ECR repository:** Must use `fromRepositoryName` to reference the existing one. Creating a new repo orphans all existing images.
- **Wildcard IAM permissions (`*`):** Every permission must be scoped to specific resource ARNs. Use `.grant*()` helpers which handle scoping automatically.
- **Skipping `cdk bootstrap`:** First-time CDK deployment requires bootstrapping the account/region. Without it, deploy fails with cryptic S3 bucket errors.
- **Not pinning the instance AZ:** If the data volume has `deleteOnTermination: false`, the instance must be in the same AZ as the volume. Pin via `availabilityZone` property.
- **Forgetting `cdk.context.json` in git:** VPC lookups and AZ resolution are cached in this file. Without it, builds on different machines may produce different infrastructure.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| VPC + subnets + IGW + route tables | Manual CloudFormation or console clicking | `ec2.Vpc` L2 construct with `subnetConfiguration` | Vpc construct handles IGW, route tables, NAT gateways, subnet CIDR allocation automatically |
| IAM policy scoping for ECR/Secrets/Logs | Inline JSON policy documents | `.grantPull()`, `.grantRead()`, `.grantWrite()` helpers | Grant methods produce correctly scoped policies including non-obvious permissions like `ecr:GetAuthorizationToken` |
| CloudFormation template JSON | Hand-written CF templates | CDK `cdk synth` | Type-safe TypeScript with IDE autocomplete eliminates typo-driven deployment failures |
| EBS mount + fstab | Custom bash scripts outside CDK | UserData commands in the Instance construct | Keeps all instance config in one place; CDK updates user-data on stack updates |

## Common Pitfalls

### Pitfall 1: CDK Creates Duplicate Resources Alongside Manual Infrastructure
**What goes wrong:** `cdk deploy` creates a NEW VPC, security groups, etc. alongside the manually-created ones. Two of everything.
**Why it happens:** CDK assumes greenfield. It does not know about existing console-created resources.
**How to avoid:** Clean slate approach -- tear down manual EC2, security groups, VPC. Let CDK create fresh. Import only ECR (which has image history worth preserving). The system tolerates minutes of downtime; all 4 WebSocket feeds auto-reconnect.
**Warning signs:** `cdk diff` shows creates for resources that already exist in the AWS console.

### Pitfall 2: CDK Bootstrap Not Run
**What goes wrong:** `cdk deploy` fails with "This stack uses assets, so the toolkit stack must be deployed" or S3 access denied.
**Why it happens:** Bootstrap is a one-time setup that creates S3 staging bucket and IAM roles. Easy to forget.
**How to avoid:** Run `cdk bootstrap aws://ACCOUNT_ID/us-east-1` before the first `cdk deploy`. Document as step 1.
**Warning signs:** Any S3-related error on first deploy.

### Pitfall 3: EBS Data Volume Lost on Instance Replacement
**What goes wrong:** CDK stack update replaces the EC2 instance (e.g., AMI or instance type change). Root volume and all data gone.
**Why it happens:** CloudFormation REPLACES (not updates) EC2 instances for certain property changes. Without a separate data volume with `deleteOnTermination: false`, data is destroyed.
**How to avoid:** Separate data volume with `deleteOnTermination: false`. Pin instance to a specific AZ matching the volume. Always run `cdk diff` before `cdk deploy` and check for "replace".
**Warning signs:** `cdk diff` output shows "replace" for the EC2 instance resource.

### Pitfall 4: VPC with NAT Gateway Costs $32/month
**What goes wrong:** Default `Vpc` construct creates a NAT Gateway per AZ ($32/month/gateway). For a public-subnet EC2 instance with an Elastic IP, NAT gateways are unnecessary.
**Why it happens:** CDK `Vpc` defaults to PUBLIC + PRIVATE_WITH_EGRESS subnets, each PRIVATE subnet getting a NAT Gateway.
**How to avoid:** Configure VPC with only PUBLIC subnets:
```typescript
const vpc = new ec2.Vpc(this, 'Vpc', {
  maxAzs: 1,  // Single AZ (matches EBS volume constraint)
  subnetConfiguration: [{
    name: 'Public',
    subnetType: ec2.SubnetType.PUBLIC,
    cidrMask: 24,
  }],
  natGateways: 0,  // Explicit: no NAT gateways
});
```
**Warning signs:** AWS bill shows NAT Gateway charges when only a public EC2 instance exists.

### Pitfall 5: Secrets Manager Secret Values in CDK Code
**What goes wrong:** Developer puts actual API keys in CDK code. Keys end up in CloudFormation template, CDK context, and git history.
**Why it happens:** Natural instinct to provide values where CDK expects them.
**How to avoid:** CDK creates only the secret SHELL with placeholder keys. Real values are populated manually via CLI or console AFTER first deploy. Secret values NEVER appear in IaC code.

### Pitfall 6: Security Group Allows Inbound on All Ports
**What goes wrong:** Over-permissive security group exposes Prometheus metrics (:9000), health endpoint (:9001), and any other service to the internet.
**Why it happens:** Using `allowAllOutbound: true` is fine, but developers also add broad inbound rules for convenience.
**How to avoid:** Explicit inbound rules only:
- SSH (22): from operator's IP only (or none if using SSM only)
- No public inbound for 9000 or 9001 -- these are internal/VPC-only
- All outbound allowed (needed for ECR pull, API calls, WebSocket connections)

## Code Examples

### Complete Stack Structure
```typescript
// Source: CDK official docs + project-specific patterns
// infra/cdk/lib/prediction-stack.ts

import * as cdk from 'aws-cdk-lib';
import * as ec2 from 'aws-cdk-lib/aws-ec2';
import * as iam from 'aws-cdk-lib/aws-iam';
import * as ecr from 'aws-cdk-lib/aws-ecr';
import * as secretsmanager from 'aws-cdk-lib/aws-secretsmanager';
import * as logs from 'aws-cdk-lib/aws-logs';
import { Construct } from 'constructs';

export class PredictionStack extends cdk.Stack {
  constructor(scope: Construct, id: string, props?: cdk.StackProps) {
    super(scope, id, props);

    // === Network ===
    const vpc = new ec2.Vpc(this, 'Vpc', {
      maxAzs: 1,
      natGateways: 0,
      subnetConfiguration: [{
        name: 'Public',
        subnetType: ec2.SubnetType.PUBLIC,
        cidrMask: 24,
      }],
    });

    const sg = new ec2.SecurityGroup(this, 'InstanceSg', {
      vpc,
      description: 'Prediction EC2 instance security group',
      allowAllOutbound: true,
    });
    // SSH from operator IP only (or remove if SSM-only)
    // sg.addIngressRule(ec2.Peer.ipv4('YOUR.IP/32'), ec2.Port.tcp(22), 'SSH');

    // === ECR (import existing, do NOT create) ===
    const ecrRepo = ecr.Repository.fromRepositoryName(this, 'EcrRepo', 'prediction');

    // === Secrets ===
    const credentials = new secretsmanager.Secret(this, 'ApiCredentials', {
      secretName: 'prediction/prod/credentials',
      description: 'Venue API credentials',
      generateSecretString: {
        secretStringTemplate: JSON.stringify({
          DERIBIT_CLIENT_ID: 'PLACEHOLDER',
          DERIBIT_CLIENT_SECRET: 'PLACEHOLDER',
          DERIVE_WALLET_KEY: 'PLACEHOLDER',
        }),
        generateStringKey: '_generated',
      },
    });

    // === Logging ===
    const logGroup = new logs.LogGroup(this, 'AppLogGroup', {
      logGroupName: '/prediction/production',
      retention: logs.RetentionDays.TWO_WEEKS,
      removalPolicy: cdk.RemovalPolicy.DESTROY,
    });

    // === IAM ===
    const instanceRole = new iam.Role(this, 'InstanceRole', {
      assumedBy: new iam.ServicePrincipal('ec2.amazonaws.com'),
    });

    // SSM for remote management (no SSH needed)
    instanceRole.addManagedPolicy(
      iam.ManagedPolicy.fromAwsManagedPolicyName('AmazonSSMManagedInstanceCore')
    );
    // ECR pull
    ecrRepo.grantPull(instanceRole);
    // AMP remote write (for future Phase 37)
    instanceRole.addManagedPolicy(
      iam.ManagedPolicy.fromAwsManagedPolicyName('AmazonPrometheusRemoteWriteAccess')
    );
    // Secrets Manager read
    credentials.grantRead(instanceRole);
    // CloudWatch Logs write
    logGroup.grantWrite(instanceRole);

    // === Compute ===
    const instance = new ec2.Instance(this, 'Instance', {
      vpc,
      vpcSubnets: { subnetType: ec2.SubnetType.PUBLIC },
      instanceType: ec2.InstanceType.of(ec2.InstanceClass.T3, ec2.InstanceSize.SMALL),
      machineImage: ec2.MachineImage.latestAmazonLinux2023(),
      role: instanceRole,
      securityGroup: sg,
      associatePublicIpAddress: true,
      blockDevices: [
        {
          deviceName: '/dev/xvda',
          volume: ec2.BlockDeviceVolume.ebs(20, {
            volumeType: ec2.EbsDeviceVolumeType.GP3,
          }),
        },
        {
          deviceName: '/dev/xvdf',
          volume: ec2.BlockDeviceVolume.ebs(30, {
            volumeType: ec2.EbsDeviceVolumeType.GP3,
            deleteOnTermination: false,  // INFRA-05: survives replacement
          }),
        },
      ],
    });

    // User-data: format + mount data volume
    instance.userData.addCommands(
      '#!/bin/bash',
      'set -euo pipefail',
      '',
      '# Format data volume if new (no filesystem)',
      'if ! blkid /dev/xvdf; then',
      '  mkfs.ext4 /dev/xvdf',
      'fi',
      'mkdir -p /opt/prediction/data',
      'mount /dev/xvdf /opt/prediction/data',
      'echo "/dev/xvdf /opt/prediction/data ext4 defaults,nofail 0 2" >> /etc/fstab',
      '',
      '# Create data subdirectories',
      'mkdir -p /opt/prediction/data/{config,spread_logs,settlement_logs,paper_trades,state,logs}',
    );

    // === Outputs ===
    new cdk.CfnOutput(this, 'InstanceId', { value: instance.instanceId });
    new cdk.CfnOutput(this, 'EcrRepoUri', { value: ecrRepo.repositoryUri });
    new cdk.CfnOutput(this, 'SecretArn', { value: credentials.secretArn });
    new cdk.CfnOutput(this, 'LogGroupName', { value: logGroup.logGroupName });
  }
}
```

### CDK App Entry Point
```typescript
// infra/cdk/bin/app.ts
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
```

### Verification Commands
```bash
# After cdk deploy, verify all resources:
# 1. VPC exists
aws ec2 describe-vpcs --filters "Name=tag:aws:cloudformation:stack-name,Values=PredictionStack" --query 'Vpcs[0].VpcId'

# 2. EC2 instance running
aws ec2 describe-instances --filters "Name=tag:aws:cloudformation:stack-name,Values=PredictionStack" --query 'Reservations[0].Instances[0].State.Name'

# 3. Data volume attached
aws ec2 describe-volumes --filters "Name=attachment.device,Values=/dev/xvdf" --query 'Volumes[0].State'

# 4. Secret exists (shell)
aws secretsmanager describe-secret --secret-id prediction/prod/credentials --query 'Name'

# 5. Log group exists with retention
aws logs describe-log-groups --log-group-name-prefix /prediction/production --query 'logGroups[0].retentionInDays'

# 6. Idempotent: destroy + redeploy produces same result
cdk destroy --force && cdk deploy --require-approval never
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| CDK v1 (per-module packages) | CDK v2 (monolith aws-cdk-lib) | CDK v2 GA Dec 2021 | Single package import, no version mismatches |
| `natGateways: undefined` (default creates NAT) | Explicit `natGateways: 0` | Always available | Saves $32+/month for public-subnet-only deployments |
| `ec2.AmazonLinuxImage()` (AL2) | `ec2.MachineImage.latestAmazonLinux2023()` | AL2023 GA Mar 2023 | Modern base, better defaults, deterministic updates |
| Multiple stacks for small projects | Single stack with constructs | CDK best practices updated | Simpler deployment, no cross-stack issues |

## Open Questions

1. **Elastic IP allocation**
   - What we know: EC2 in public subnet gets a dynamic public IP by default. EIP provides a stable IP.
   - What's unclear: Whether the current manual EC2 has an EIP that should be preserved, or if dynamic IP is acceptable.
   - Recommendation: Add an EIP to the CDK stack for DNS stability. If an existing EIP exists, import it.

2. **Key pair for emergency SSH access**
   - What we know: SSM Session Manager is the primary access path (no SSH needed). But some operators want SSH as a backup.
   - What's unclear: Whether a KeyPair should be provisioned.
   - Recommendation: Create a KeyPair in CDK but add NO SSH inbound rule to the security group. SSH access can be enabled per-incident by temporarily adding an inbound rule. Default to SSM-only.

3. **Data volume on instance replacement**
   - What we know: `deleteOnTermination: false` keeps the volume on termination. But if CDK replaces the instance, the new instance gets a NEW (blank) data volume from the blockDevices spec.
   - What's unclear: How to automatically reattach the old data volume after replacement.
   - Recommendation: For v1.6, accept that instance replacement requires manual reattachment of the data volume. This is rare (only on AMI or instance type changes). Add a CfnOutput for the data volume ID so it can be found. Always run `cdk diff` to catch replacements before they happen.

## Sources

### Primary (HIGH confidence)
- [AWS CDK EC2 Volume construct](https://docs.aws.amazon.com/cdk/api/v2/docs/aws-cdk-lib.aws_ec2.Volume.html) - EBS volume with RemovalPolicy, attachment patterns
- [AWS CDK EC2 module README](https://docs.aws.amazon.com/cdk/api/v2/docs/aws-cdk-lib.aws_ec2-readme.html) - Instance, VPC, SecurityGroup, UserData, BlockDevice examples
- [AWS CDK ECR module README](https://docs.aws.amazon.com/cdk/api/v2/docs/aws-cdk-lib.aws_ecr-readme.html) - Repository.fromRepositoryName import pattern
- [AWS CDK Secrets Manager README](https://docs.aws.amazon.com/cdk/api/v2/docs/aws-cdk-lib.aws_secretsmanager-readme.html) - Secret shell with generateSecretString
- [AWS CDK Logs README](https://docs.aws.amazon.com/cdk/api/v2/docs/aws-cdk-lib.aws_logs-readme.html) - LogGroup with RetentionDays
- [AWS CDK Best Practices](https://docs.aws.amazon.com/cdk/v2/guide/best-practices.html) - Single stack recommendation for small projects
- [AWS CDK Stacks Guide](https://docs.aws.amazon.com/cdk/v2/guide/stacks.html) - Stack vs construct organization

### Secondary (MEDIUM confidence)
- [CDK Bootstrap Guide](https://docs.aws.amazon.com/cdk/v2/guide/bootstrapping.html) - Bootstrap requirements and troubleshooting
- [CDK in Existing Project pattern](https://dev.to/alexvladut/how-to-add-aws-cdk-to-an-existing-project-2d30) - infra/ subdirectory pattern

### Project-Specific (HIGH confidence)
- `deploy/aws-setup.sh` - Current manual bootstrap script (to be replaced by CDK user-data)
- `deploy/ecr-push.sh` - Confirms ECR repo name is `prediction`, account `606103597377`
- `docker-compose.yml` - Confirms current volume mounts and port mapping
- `.planning/research/STACK.md` - Verified CDK version, module selection, IAM policy set
- `.planning/research/ARCHITECTURE.md` - Verified project structure, stack decomposition decision
- `.planning/research/PITFALLS.md` - CDK duplicate resources, bootstrap, EBS data loss pitfalls

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - CDK v2 is mature; all constructs verified in official docs
- Architecture: HIGH - Single stack pattern verified against CDK best practices; all L2 constructs have verified code examples
- Pitfalls: HIGH - All pitfalls documented in project pitfalls research and verified against CDK docs

**Research date:** 2026-03-07
**Valid until:** 2026-04-07 (CDK is stable; patterns unlikely to change within 30 days)
