import express from "express";
import cors from "cors";
import { clerkMiddleware } from "@clerk/express";
import http from "http";
import processRoutes from "./src/routes/process-routes";
import healthRoutes from "./src/routes/health-routes";
import apiKeyRoutes from "./src/routes/apiKey-routes";
import subscriptionRoutes from "./src/routes/subscription-routes";
import stripeRoutes from "./src/routes/stripe-routes";
import usageRoutes from "./src/routes/usage-routes";
import apiProcessRoutes from "./src/routes/api/process-routes"; // Added
import https from "https";
import fs from "fs";
import { apiLimiter } from "./src/middleware/rateLimiters";
import { handleStripeWebhook } from "./src/controllers/stripe-webhook-controller";

const app = express();
const port = process.env.PORT || 9001;

const trustProxyEnv = process.env.TRUST_PROXY;
if (trustProxyEnv !== undefined) {
	const normalized = trustProxyEnv.trim().toLowerCase();
	if (normalized === "true") {
		app.set("trust proxy", true);
	} else if (normalized === "false") {
		app.set("trust proxy", false);
	} else if (/^\d+$/.test(normalized)) {
		app.set("trust proxy", Number.parseInt(normalized, 10));
	} else {
		app.set("trust proxy", trustProxyEnv);
	}
} else {
	// Coolify runs behind a reverse proxy; trust a single hop by default.
	app.set("trust proxy", 1);
}

app.use(cors({
	origin: '*',
	methods: ['GET', 'POST', 'PATCH', 'DELETE'],
	allowedHeaders: ['Origin', 'X-Requested-With', 'Content-Type', 'Accept', 'Authorization', 'X-API-Key']
}));

// Stripe webhooks require the raw request body for signature verification.
app.post("/api/stripe/webhook", express.raw({ type: "application/json" }), handleStripeWebhook);

// Middleware to parse JSON request bodies
app.use(express.json());

app.use((err: any, req: any, res: any, next: any) => {
	console.error(err.stack);
	res.status(401).send("Unauthenticated!");
});

// Routes
app.use("/health", healthRoutes);
app.use("/process", processRoutes);

const apiRouter = express.Router();
apiRouter.use(apiLimiter);
apiRouter.use("/keys", apiKeyRoutes);
apiRouter.use("/subscription", subscriptionRoutes);
apiRouter.use("/stripe", stripeRoutes);
apiRouter.use("/usage", usageRoutes);
apiRouter.use("/process", apiProcessRoutes); // Added

app.use("/api", apiRouter);

app.use((req, res) => {
	res.status(404).send("Not Found");
});

// TLS configuration
const keyPath = process.env.TLS_KEY_PATH;
const certPath = process.env.TLS_CERT_PATH;

let server;

if (keyPath && certPath && fs.existsSync(keyPath) && fs.existsSync(certPath)) {
	const options = {
		key: fs.readFileSync(keyPath),
		cert: fs.readFileSync(certPath),
	};
	server = https.createServer(options, app);
	console.log("TLS configuration loaded. Running in HTTPS mode.");
} else {
	server = http.createServer(app);
	if (keyPath || certPath) {
		if (!keyPath || !fs.existsSync(keyPath)) {
			console.error(`TLS key file not found at: ${keyPath}`);
		}
		if (!certPath || !fs.existsSync(certPath)) {
			console.error(`TLS certificate file not found at: ${certPath}`);
		}
		console.error("Proceeding without TLS.");
	} else {
		console.log("TLS environment variables not set. Running in HTTP mode.");
	}
}

server.listen(port, "0.0.0.0", () => {
	console.log(`Server is running over ${port - 1}`);
});
